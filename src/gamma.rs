//! Tints every screen by setting its gamma ramp: the table the display
//! hardware passes each colour value through on its way out.
//!
//! Each screen's own ramp is read the first time Luz sees it and kept as its
//! base, so a calibration curve loaded by the OS or a colorimeter is tinted
//! rather than thrown away, and put back on `restore`. A base that is already
//! tinted (Luz or f.lux was killed mid-evening) is not taken for calibration:
//! it is replaced with a straight line.
//!
//! What this can't reach: HDR displays on Windows, which bypass the ramp, and
//! anything the OS paints on top of it (Night Light, Night Shift). The OS
//! also drops ramps now and then (sleep, a resolution change, a game), which
//! is why the screen thread sets them again every few seconds.

use crate::color::Gains;

pub use imp::{restore, set, unlock_warmer, warmer_unlocked};

/// Entries per channel. 256 is what Windows accepts and what macOS uses.
pub const SIZE: usize = 256;

/// One channel of a ramp, 0 to 1.
pub type Curve = [f32; SIZE];

pub fn identity() -> [Curve; 3] {
    let line: Curve = std::array::from_fn(|i| i as f32 / (SIZE - 1) as f32);
    [line; 3]
}

/// A ramp as found, or `None` if it looks tinted rather than calibrated:
/// a channel that tops out well short of full, or channels far apart.
pub fn sane_base(ramp: [Curve; 3]) -> Option<[Curve; 3]> {
    let tops = ramp.map(|c| c[SIZE - 1]);
    let lo = tops.iter().cloned().fold(f32::MAX, f32::min);
    let hi = tops.iter().cloned().fold(f32::MIN, f32::max);
    let rising = ramp.iter().all(|c| c[SIZE - 1] > c[0]);
    (rising && lo >= 0.85 && hi - lo <= 0.06).then_some(ramp)
}

/// `base` tinted by `gains` and dimmed by `brightness`.
pub fn tinted(base: &[Curve; 3], gains: Gains, brightness: f64) -> [Curve; 3] {
    std::array::from_fn(|c| {
        let k = (gains[c] * brightness).clamp(0.0, 1.0) as f32;
        base[c].map(|v| (v * k).clamp(0.0, 1.0))
    })
}

/// What setting the ramps did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Screens that took the ramp.
    pub screens: usize,
    /// Screens that refused it. On Windows that is usually its limit on how
    /// far a ramp may stray from a straight line; see `unlock_warmer`.
    pub refused: usize,
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod imp {
    use super::{identity, sane_base, tinted, Curve, Outcome, SIZE};
    use crate::color::Gains;
    use crate::log::lwarn;
    use crate::util::{pcwstr, wide};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, EnumDisplayDevicesW, DISPLAY_DEVICEW,
        DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, HDC,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE,
    };
    use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};

    /// Each screen's ramp before Luz touched it, by device name.
    static BASES: Mutex<Option<HashMap<String, [Curve; 3]>>> = Mutex::new(None);

    /// Windows refuses ramps that stray far from a straight line, which stops
    /// most screens somewhere around 3000-4000 K. This value lifts the limit;
    /// f.lux sets the same one.
    const ICM_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ICM";
    const ICM_VALUE: &str = "GdiIcmGammaRange";

    /// A device context for each screen on the desktop, with its name.
    fn screens() -> Vec<(String, HDC)> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut dd = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
                break;
            }
            if (dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP).0 == 0 {
                continue;
            }
            let len = dd.DeviceName.iter().position(|&c| c == 0).unwrap_or(32);
            let name = String::from_utf16_lossy(&dd.DeviceName[..len]);
            let dc = unsafe {
                CreateDCW(
                    PCWSTR::null(),
                    PCWSTR(dd.DeviceName.as_ptr()),
                    PCWSTR::null(),
                    None,
                )
            };
            if !dc.is_invalid() {
                out.push((name, dc));
            }
        }
        out
    }

    fn read(dc: HDC) -> Option<[Curve; 3]> {
        let mut raw = [0u16; 3 * SIZE];
        if !unsafe { GetDeviceGammaRamp(dc, raw.as_mut_ptr().cast()) }.as_bool() {
            return None;
        }
        Some(std::array::from_fn(|c| {
            std::array::from_fn(|i| raw[c * SIZE + i] as f32 / 65535.0)
        }))
    }

    fn write(dc: HDC, ramp: &[Curve; 3]) -> bool {
        let mut raw = [0u16; 3 * SIZE];
        for c in 0..3 {
            for i in 0..SIZE {
                raw[c * SIZE + i] = (ramp[c][i] * 65535.0).round() as u16;
            }
        }
        unsafe { SetDeviceGammaRamp(dc, raw.as_ptr().cast()) }.as_bool()
    }

    /// Tint every screen. A screen seen for the first time has its own ramp
    /// read first, as its base.
    pub fn set(gains: Gains, brightness: f64) -> Outcome {
        let mut bases = BASES.lock().unwrap_or_else(|e| e.into_inner());
        let bases = bases.get_or_insert_with(HashMap::new);
        let mut out = Outcome::default();
        for (name, dc) in screens() {
            let base = bases
                .entry(name)
                .or_insert_with(|| read(dc).and_then(sane_base).unwrap_or_else(identity));
            if write(dc, &tinted(base, gains, brightness)) {
                out.screens += 1;
            } else {
                out.refused += 1;
            }
            unsafe {
                let _ = DeleteDC(dc);
            }
        }
        out
    }

    /// Put every screen back as it was found (or straight, if never seen).
    pub fn restore() {
        let bases = BASES.lock().unwrap_or_else(|e| e.into_inner());
        for (name, dc) in screens() {
            let base = bases
                .as_ref()
                .and_then(|b| b.get(&name).copied())
                .unwrap_or_else(identity);
            if !write(dc, &base) {
                lwarn!("could not put back the ramp on {name}");
            }
            unsafe {
                let _ = DeleteDC(dc);
            }
        }
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    /// Whether Windows has been told to accept ramps of any shape.
    pub fn warmer_unlocked() -> bool {
        let sub = wide(ICM_KEY);
        let name = wide(ICM_VALUE);
        let mut hkey = HKEY::default();
        unsafe {
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                pcwstr(&sub),
                None,
                KEY_QUERY_VALUE,
                &mut hkey,
            ) != ERROR_SUCCESS
            {
                return false;
            }
            let key = Key(hkey);
            let mut data = 0u32;
            let mut size = 4u32;
            let rc = RegQueryValueExW(
                key.0,
                pcwstr(&name),
                None,
                None,
                Some((&mut data as *mut u32).cast()),
                Some(&mut size),
            );
            rc == ERROR_SUCCESS && data >= 256
        }
    }

    /// Tell Windows to accept ramps of any shape. Needs an administrator,
    /// and takes effect after the next sign-in.
    pub fn unlock_warmer() -> Result<(), String> {
        let sub = wide(ICM_KEY);
        let name = wide(ICM_VALUE);
        let mut hkey = HKEY::default();
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                pcwstr(&sub),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut hkey,
                None,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(format!("could not open HKLM\\{ICM_KEY} (error {})", rc.0));
        }
        let key = Key(hkey);
        let value = 256u32.to_le_bytes();
        let rc = unsafe { RegSetValueExW(key.0, pcwstr(&name), None, REG_DWORD, Some(&value)) };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("could not set {ICM_VALUE} (error {})", rc.0))
        }
    }
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod imp {
    use super::{identity, sane_base, tinted, Curve, Outcome, SIZE};
    use crate::color::Gains;
    use std::collections::HashMap;
    use std::sync::Mutex;

    type DisplayId = u32;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetOnlineDisplayList(max: u32, displays: *mut DisplayId, count: *mut u32) -> i32;
        fn CGGetDisplayTransferByTable(
            display: DisplayId,
            capacity: u32,
            red: *mut f32,
            green: *mut f32,
            blue: *mut f32,
            count: *mut u32,
        ) -> i32;
        fn CGSetDisplayTransferByTable(
            display: DisplayId,
            size: u32,
            red: *const f32,
            green: *const f32,
            blue: *const f32,
        ) -> i32;
        fn CGDisplayRestoreColorSyncSettings();
    }

    static BASES: Mutex<Option<HashMap<DisplayId, [Curve; 3]>>> = Mutex::new(None);

    fn screens() -> Vec<DisplayId> {
        let mut ids = [0 as DisplayId; 16];
        let mut n = 0u32;
        if unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) } != 0 {
            return Vec::new();
        }
        ids[..n as usize].to_vec()
    }

    fn read(id: DisplayId) -> Option<[Curve; 3]> {
        let mut t = [[0f32; SIZE]; 3];
        let mut n = 0u32;
        let [r, g, b] = &mut t;
        let rc = unsafe {
            CGGetDisplayTransferByTable(
                id,
                SIZE as u32,
                r.as_mut_ptr(),
                g.as_mut_ptr(),
                b.as_mut_ptr(),
                &mut n,
            )
        };
        // A table of another length would need resampling; a straight line
        // is the better guess than a wrong curve.
        (rc == 0 && n as usize == SIZE).then_some(t)
    }

    fn write(id: DisplayId, ramp: &[Curve; 3]) -> bool {
        let [r, g, b] = ramp;
        unsafe {
            CGSetDisplayTransferByTable(id, SIZE as u32, r.as_ptr(), g.as_ptr(), b.as_ptr()) == 0
        }
    }

    pub fn set(gains: Gains, brightness: f64) -> Outcome {
        let mut bases = BASES.lock().unwrap_or_else(|e| e.into_inner());
        let bases = bases.get_or_insert_with(HashMap::new);
        let mut out = Outcome::default();
        for id in screens() {
            let base = bases
                .entry(id)
                .or_insert_with(|| read(id).and_then(sane_base).unwrap_or_else(identity));
            if write(id, &tinted(base, gains, brightness)) {
                out.screens += 1;
            } else {
                out.refused += 1;
            }
        }
        out
    }

    /// ColorSync's own tables, as if Luz had never run. macOS does this by
    /// itself when Luz quits, too.
    pub fn restore() {
        unsafe { CGDisplayRestoreColorSyncSettings() };
    }

    /// macOS has no limit to lift.
    pub fn warmer_unlocked() -> bool {
        true
    }

    pub fn unlock_warmer() -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_and_calibrated_ramps_are_bases_tinted_ones_are_not() {
        assert!(sane_base(identity()).is_some());
        let mut calibrated = identity();
        calibrated[2] = calibrated[2].map(|v| v * 0.97);
        assert!(sane_base(calibrated).is_some());
        let warm = tinted(&identity(), crate::color::gains(3400.0), 1.0);
        assert!(sane_base(warm).is_none());
        assert!(sane_base([[0.0; SIZE]; 3]).is_none());
    }

    #[test]
    fn tinting_scales_each_channel() {
        let t = tinted(&identity(), [1.0, 0.5, 0.0], 0.8);
        assert!((t[0][SIZE - 1] - 0.8).abs() < 1e-6);
        assert!((t[1][SIZE - 1] - 0.4).abs() < 1e-6);
        assert_eq!(t[2][SIZE - 1], 0.0);
        assert_eq!(t[0][0], 0.0);
    }
}

/// Tints this machine's screens for a moment: `cargo test -- --ignored`.
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore = "changes the screen's colour for a moment"]
    fn how_warm_this_machine_goes() {
        for k in [4500, 3400, 2700, 1900, 1200] {
            let out = set(crate::color::gains(k as f64), 1.0);
            eprintln!("{k} K: {out:?}");
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        restore();
        eprintln!("unlocked: {}", warmer_unlocked());
    }
}
