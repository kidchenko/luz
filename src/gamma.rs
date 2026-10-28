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
//!
//! On Linux the ramps are X11's, through RandR. A Wayland session keeps them
//! to the compositor, so on GNOME Luz drives GNOME's own Night Light instead,
//! and elsewhere on Wayland it can't tint at all.

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
    use std::collections::HashMap;
    use std::sync::Mutex;
    use traykit::lwarn;
    use traykit::util::{pcwstr, wide};
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
    pub fn set(_kelvin: f64, gains: Gains, brightness: f64) -> Outcome {
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

    pub fn set(_kelvin: f64, gains: Gains, brightness: f64) -> Outcome {
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

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
mod imp {
    use super::Outcome;
    use crate::color::Gains;
    use std::sync::{Mutex, OnceLock};
    use traykit::{linfo, lwarn};

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Backend {
        X11,
        Wlr,
        NightLight(night_light::Desktop),
        None,
    }

    /// Decided once: the session doesn't change under a running app. GNOME
    /// and KDE keep their Wayland ramps to themselves, but have a Night Light
    /// Luz can drive; wlroots compositors (Hyprland, Sway, river, niri,
    /// labwc) hand the ramps out.
    fn backend() -> Backend {
        static BACKEND: OnceLock<Backend> = OnceLock::new();
        *BACKEND.get_or_init(|| {
            let var = |k| std::env::var(k).unwrap_or_default();
            let wayland =
                var("XDG_SESSION_TYPE") == "wayland" || !var("WAYLAND_DISPLAY").is_empty();
            let b = if !wayland && !var("DISPLAY").is_empty() {
                Backend::X11
            } else if let Some(d) = wayland.then(night_light::Desktop::detect).flatten() {
                Backend::NightLight(d)
            } else if wayland && wlr::available() {
                Backend::Wlr
            } else {
                Backend::None
            };
            match b {
                Backend::X11 => linfo!("tinting through X11 gamma ramps"),
                Backend::Wlr => linfo!("tinting through Wayland gamma control"),
                Backend::NightLight(d) => linfo!("tinting through {}", d.name()),
                Backend::None => lwarn!("this desktop has no way for Luz to tint the screen"),
            }
            b
        })
    }

    pub fn set(kelvin: f64, gains: Gains, brightness: f64) -> Outcome {
        match backend() {
            Backend::X11 => x11::set(gains, brightness),
            Backend::Wlr => wlr::set(gains, brightness),
            Backend::NightLight(d) => night_light::set(d, kelvin),
            Backend::None => Outcome::default(),
        }
    }

    pub fn restore() {
        match backend() {
            Backend::X11 => x11::restore(),
            Backend::Wlr => wlr::restore(),
            Backend::NightLight(d) => night_light::restore(d),
            Backend::None => {}
        }
    }

    /// Linux has no limit to lift.
    pub fn warmer_unlocked() -> bool {
        true
    }

    pub fn unlock_warmer() -> Result<(), String> {
        Ok(())
    }

    /// `curve` read at `len` evenly spaced points: X11 and Wayland ramps come
    /// in whatever length the driver likes (256 and 1024 are both common).
    fn resample(curve: &[f32], len: usize) -> Vec<f32> {
        let last = (curve.len() - 1) as f32;
        (0..len)
            .map(|i| {
                let x = i as f32 * last / (len - 1).max(1) as f32;
                let (lo, t) = (x.floor() as usize, x.fract());
                let hi = (lo + 1).min(curve.len() - 1);
                curve[lo] + (curve[hi] - curve[lo]) * t
            })
            .collect()
    }

    /// RandR keeps a ramp per CRTC, of whatever length the driver likes
    /// (256 and 1024 are both common), as 16-bit values.
    mod x11 {
        use super::super::{identity, sane_base, tinted, Curve, Outcome, SIZE};
        use super::{resample, Mutex};
        use crate::color::Gains;
        use std::collections::HashMap;
        use x11rb::connection::Connection;
        use x11rb::protocol::randr::{ConnectionExt as _, Crtc};
        use x11rb::rust_connection::RustConnection;

        struct State {
            conn: RustConnection,
            root: u32,
            /// Each CRTC's ramp as first found, at Luz's 256 entries.
            bases: HashMap<Crtc, [Curve; 3]>,
        }

        static STATE: Mutex<Option<State>> = Mutex::new(None);

        fn connect() -> Option<State> {
            let (conn, n) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(n)?.root;
            Some(State {
                conn,
                root,
                bases: HashMap::new(),
            })
        }

        /// Lit CRTCs (ones showing a mode) and their ramp lengths.
        fn crtcs(st: &State) -> Option<Vec<(Crtc, usize)>> {
            let res = st
                .conn
                .randr_get_screen_resources_current(st.root)
                .ok()?
                .reply()
                .ok()?;
            let mut out = Vec::new();
            for crtc in res.crtcs {
                let info = st
                    .conn
                    .randr_get_crtc_info(crtc, res.config_timestamp)
                    .ok()?
                    .reply()
                    .ok()?;
                if info.mode == 0 {
                    continue;
                }
                let size = st.conn.randr_get_crtc_gamma_size(crtc).ok()?.reply().ok()?;
                if size.size > 1 {
                    out.push((crtc, size.size as usize));
                }
            }
            Some(out)
        }

        fn read(st: &State, crtc: Crtc) -> Option<[Curve; 3]> {
            let g = st.conn.randr_get_crtc_gamma(crtc).ok()?.reply().ok()?;
            let channel = |v: &[u16]| -> Option<Curve> {
                let f: Vec<f32> = v.iter().map(|&x| x as f32 / 65535.0).collect();
                (f.len() > 1).then(|| resample(&f, SIZE).try_into().ok())?
            };
            Some([channel(&g.red)?, channel(&g.green)?, channel(&g.blue)?])
        }

        fn write(st: &State, crtc: Crtc, len: usize, ramp: &[Curve; 3]) -> bool {
            let [r, g, b] = ramp.each_ref().map(|c| {
                resample(c, len)
                    .into_iter()
                    .map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
                    .collect::<Vec<_>>()
            });
            st.conn
                .randr_set_crtc_gamma(crtc, &r, &g, &b)
                .is_ok_and(|c| c.check().is_ok())
        }

        pub fn set(gains: Gains, brightness: f64) -> Outcome {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            if state.is_none() {
                *state = connect();
            }
            let Some(st) = state.as_mut() else {
                return Outcome::default();
            };
            let Some(crtcs) = crtcs(st) else {
                // The X server went away or restarted: connect again next
                // time, and take the ramps as found then.
                *state = None;
                return Outcome::default();
            };
            let mut out = Outcome::default();
            for (crtc, len) in crtcs {
                if !st.bases.contains_key(&crtc) {
                    let base = read(st, crtc).and_then(sane_base).unwrap_or_else(identity);
                    st.bases.insert(crtc, base);
                }
                if write(st, crtc, len, &tinted(&st.bases[&crtc], gains, brightness)) {
                    out.screens += 1;
                } else {
                    out.refused += 1;
                }
            }
            out
        }

        /// The ramps as Luz found them.
        pub fn restore() {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            let Some(st) = state.as_ref() else { return };
            for (crtc, len) in crtcs(st).unwrap_or_default() {
                if let Some(base) = st.bases.get(&crtc) {
                    write(st, crtc, len, base);
                }
            }
            let _ = st.conn.flush();
            *state = None;
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn resampling_keeps_the_ends_and_the_line() {
                let line: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
                let long = resample(&line, 1024);
                assert_eq!(long.len(), 1024);
                assert_eq!(long[0], 0.0);
                assert!((long[1023] - 1.0).abs() < 1e-6);
                assert!((long[512] - 512.0 / 1023.0).abs() < 1e-3);
                let back = resample(&long, 256);
                assert!(back.iter().zip(&line).all(|(a, b)| (a - b).abs() < 1e-4));
            }
        }
    }

    /// wlroots' gamma control: a ramp per output, handed over in a memfd of
    /// 16-bit values. The compositor can't be asked for the ramp it had, so
    /// the base is a straight line (as for wlsunset and gammastep), and it
    /// puts its own back by itself once Luz lets go, crash or not.
    mod wlr {
        use super::super::{identity, tinted, Curve, Outcome};
        use super::{resample, Mutex};
        use crate::color::Gains;
        use std::fs::File;
        use std::io::{Seek, Write};
        use std::os::fd::{AsFd, FromRawFd};
        use std::time::{Duration, Instant};
        use traykit::{linfo, lwarn};
        use wayland_client::protocol::wl_output::{self, WlOutput};
        use wayland_client::protocol::wl_registry::{self, WlRegistry};
        use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
        use wayland_protocols_wlr::gamma_control::v1::client::{
            zwlr_gamma_control_manager_v1::{self, ZwlrGammaControlManagerV1},
            zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
        };

        /// How long to leave a screen another app holds (wlsunset, gammastep)
        /// before asking for it again.
        const RETRY: Duration = Duration::from_secs(10);

        struct Screen {
            /// The output's global name, which outlives the proxy.
            name: u32,
            output: WlOutput,
            control: Option<ZwlrGammaControlV1>,
            /// Ramp length, once the compositor has said.
            size: usize,
            refused_at: Option<Instant>,
        }

        #[derive(Default)]
        struct Globals {
            manager: Option<ZwlrGammaControlManagerV1>,
            screens: Vec<Screen>,
        }

        struct State {
            conn: Connection,
            queue: EventQueue<Globals>,
            globals: Globals,
        }

        static STATE: Mutex<Option<State>> = Mutex::new(None);

        fn connect() -> Option<State> {
            let conn = Connection::connect_to_env().ok()?;
            let mut queue = conn.new_event_queue();
            conn.display().get_registry(&queue.handle(), ());
            let mut globals = Globals::default();
            queue.roundtrip(&mut globals).ok()?;
            globals.manager.is_some().then_some(State {
                conn,
                queue,
                globals,
            })
        }

        /// Whether the compositor hands out gamma control at all. The
        /// connection is kept for `set`.
        pub fn available() -> bool {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            if state.is_none() {
                *state = connect();
            }
            state.is_some()
        }

        /// The ramp as a file of red, then green, then blue, `size` each.
        fn ramp_file(ramp: &[Curve; 3], size: usize) -> Option<File> {
            let fd = unsafe { libc::memfd_create(c"luz-gamma".as_ptr(), libc::MFD_CLOEXEC) };
            if fd < 0 {
                return None;
            }
            // Owned from here, so it closes whatever happens next.
            let mut file = unsafe { File::from_raw_fd(fd) };
            let bytes: Vec<u8> = ramp
                .iter()
                .flat_map(|c| resample(c, size))
                .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_ne_bytes())
                .collect();
            file.write_all(&bytes).ok()?;
            file.rewind().ok()?;
            Some(file)
        }

        pub fn set(gains: Gains, brightness: f64) -> Outcome {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            if state.is_none() {
                *state = connect();
            }
            let Some(st) = state.as_mut() else {
                return Outcome::default();
            };
            let qh = st.queue.handle();
            // Screens plugged in or out, and answers to earlier requests.
            if st.queue.roundtrip(&mut st.globals).is_err() {
                *state = None;
                return Outcome::default();
            }
            let Some(manager) = st.globals.manager.clone() else {
                return Outcome::default();
            };
            let mut asked = false;
            for s in &mut st.globals.screens {
                if s.control.is_none() && s.refused_at.is_none_or(|t| t.elapsed() >= RETRY) {
                    s.control = Some(manager.get_gamma_control(&s.output, &qh, s.name));
                    asked = true;
                }
            }
            if asked && st.queue.roundtrip(&mut st.globals).is_err() {
                *state = None;
                return Outcome::default();
            }

            let ramp = tinted(&identity(), gains, brightness);
            let mut out = Outcome::default();
            for s in &st.globals.screens {
                let Some(control) = s.control.as_ref().filter(|_| s.size > 1) else {
                    continue;
                };
                if let Some(file) = ramp_file(&ramp, s.size) {
                    control.set_gamma(file.as_fd());
                    out.screens += 1;
                }
            }
            let _ = st.conn.flush();
            out
        }

        /// Letting go is enough: the compositor puts its own ramps back.
        pub fn restore() {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(st) = state.as_mut() {
                for s in &mut st.globals.screens {
                    if let Some(c) = s.control.take() {
                        c.destroy();
                    }
                }
                let _ = st.conn.flush();
            }
            *state = None;
        }

        impl Dispatch<WlRegistry, ()> for Globals {
            fn event(
                g: &mut Self,
                registry: &WlRegistry,
                event: wl_registry::Event,
                _: &(),
                _: &Connection,
                qh: &QueueHandle<Self>,
            ) {
                match event {
                    wl_registry::Event::Global {
                        name, interface, ..
                    } => match interface.as_str() {
                        "zwlr_gamma_control_manager_v1" => {
                            g.manager = Some(registry.bind(name, 1, qh, ()));
                        }
                        "wl_output" => g.screens.push(Screen {
                            name,
                            output: registry.bind(name, 1, qh, ()),
                            control: None,
                            size: 0,
                            refused_at: None,
                        }),
                        _ => {}
                    },
                    wl_registry::Event::GlobalRemove { name } => {
                        if let Some(i) = g.screens.iter().position(|s| s.name == name) {
                            if let Some(c) = g.screens.remove(i).control {
                                c.destroy();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        impl Dispatch<ZwlrGammaControlV1, u32> for Globals {
            fn event(
                g: &mut Self,
                control: &ZwlrGammaControlV1,
                event: zwlr_gamma_control_v1::Event,
                name: &u32,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                let Some(s) = g.screens.iter_mut().find(|s| s.name == *name) else {
                    return;
                };
                match event {
                    zwlr_gamma_control_v1::Event::GammaSize { size } => s.size = size as usize,
                    // Another app has this screen, or it went away.
                    zwlr_gamma_control_v1::Event::Failed => {
                        if s.refused_at.is_none() {
                            lwarn!(
                                "a screen refused gamma control (another app has it, or the \
                                 screen can't); trying again later"
                            );
                        }
                        control.destroy();
                        s.control = None;
                        s.size = 0;
                        s.refused_at = Some(Instant::now());
                    }
                    _ => {}
                }
                if s.size > 1 && s.refused_at.take().is_some() {
                    linfo!("got a screen's gamma back");
                }
            }
        }

        impl Dispatch<WlOutput, ()> for Globals {
            fn event(
                _: &mut Self,
                _: &WlOutput,
                _: wl_output::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }

        impl Dispatch<ZwlrGammaControlManagerV1, ()> for Globals {
            fn event(
                _: &mut Self,
                _: &ZwlrGammaControlManagerV1,
                _: zwlr_gamma_control_manager_v1::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    }

    /// GNOME and KDE keep their Wayland ramps to themselves, but each has a
    /// Night Light that takes a temperature: GNOME's through gsettings,
    /// KDE's through kwinrc, which KWin watches. Luz sets it to always on at
    /// the colour it wants, and off for daylight. What the user had is saved
    /// to a file first and put back on quit, or on the next launch's quit if
    /// Luz didn't get to. Brightness can't go through either.
    pub(super) mod night_light {
        use super::Mutex;
        use std::path::PathBuf;
        use std::process::Command;
        use traykit::lwarn;

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum Desktop {
            Gnome,
            Kde,
        }

        const GNOME_SCHEMA: &str = "org.gnome.settings-daemon.plugins.color";
        const GNOME_KEYS: [&str; 5] = [
            "night-light-enabled",
            "night-light-temperature",
            "night-light-schedule-automatic",
            "night-light-schedule-from",
            "night-light-schedule-to",
        ];
        const KDE_KEYS: [&str; 3] = ["Active", "Mode", "NightTemperature"];

        /// Warmer than this and it's worth switching Night Light on.
        const OFF_ABOVE: f64 = 6450.0;

        /// What Luz last wrote, so the screen thread's frequent calls only
        /// reach the desktop when the colour has moved.
        static LAST: Mutex<Option<Option<u32>>> = Mutex::new(None);

        fn run(cmd: &str, args: &[&str]) -> Option<String> {
            let out = Command::new(cmd).args(args).output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        }

        impl Desktop {
            pub fn detect() -> Option<Self> {
                let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
                let is = |name: &str| desktop.split(':').any(|d| d.eq_ignore_ascii_case(name));
                let d = if is("GNOME") {
                    Desktop::Gnome
                } else if is("KDE") {
                    Desktop::Kde
                } else {
                    return None;
                };
                d.get(d.keys()[0]).is_some().then_some(d)
            }

            pub fn name(self) -> &'static str {
                match self {
                    Desktop::Gnome => "GNOME's Night Light",
                    Desktop::Kde => "KDE's Night Light",
                }
            }

            fn keys(self) -> &'static [&'static str] {
                match self {
                    Desktop::Gnome => &GNOME_KEYS,
                    Desktop::Kde => &KDE_KEYS,
                }
            }

            /// Each desktop's own limits on the temperature.
            fn range(self) -> (u32, u32) {
                match self {
                    Desktop::Gnome => (1000, 10000),
                    Desktop::Kde => (1000, 6500),
                }
            }

            /// A key's value as the desktop writes it, or `""` for a KDE key
            /// that isn't in kwinrc (its default applies). `None` if the
            /// desktop couldn't be asked.
            fn get(self, key: &str) -> Option<String> {
                match self {
                    Desktop::Gnome => run("gsettings", &["get", GNOME_SCHEMA, key]),
                    Desktop::Kde => run(
                        "kreadconfig6",
                        &["--file", "kwinrc", "--group", "NightColor", "--key", key],
                    ),
                }
            }

            /// Set a key, or put it back to the desktop's default for `""`.
            fn put(self, key: &str, value: &str) -> bool {
                let ok = match (self, value) {
                    (Desktop::Gnome, "") => run("gsettings", &["reset", GNOME_SCHEMA, key]),
                    (Desktop::Gnome, v) => run("gsettings", &["set", GNOME_SCHEMA, key, v]),
                    // --notify is what KWin's config watcher hears.
                    (Desktop::Kde, v) => {
                        let mut args = vec!["--file", "kwinrc", "--group", "NightColor"];
                        args.extend(["--key", key, "--notify"]);
                        if v.is_empty() {
                            args.push("--delete");
                        } else {
                            args.extend(["--type", kde_type(key), v]);
                        }
                        run("kwriteconfig6", &args)
                    }
                }
                .is_some();
                if !ok {
                    lwarn!("could not set {key} to {value:?}");
                }
                ok
            }

            fn temperature(self, k: u32) -> bool {
                match self {
                    Desktop::Gnome => self.put("night-light-temperature", &format!("uint32 {k}")),
                    Desktop::Kde => self.put("NightTemperature", &k.to_string()),
                }
            }

            /// On, all day, at `k`.
            fn on(self, k: u32) -> bool {
                let steps = match self {
                    // The same time for start and end is all day to GNOME.
                    Desktop::Gnome => vec![
                        self.put("night-light-schedule-automatic", "false"),
                        self.put("night-light-schedule-from", "0.0"),
                        self.put("night-light-schedule-to", "0.0"),
                        self.temperature(k),
                        self.put("night-light-enabled", "true"),
                    ],
                    Desktop::Kde => vec![
                        self.put("Mode", "Constant"),
                        self.temperature(k),
                        self.put("Active", "true"),
                    ],
                };
                steps.iter().all(|&ok| ok)
            }

            fn off(self) -> bool {
                match self {
                    Desktop::Gnome => self.put("night-light-enabled", "false"),
                    Desktop::Kde => self.put("Active", "false"),
                }
            }

            fn saved_path(self) -> Option<PathBuf> {
                let file = match self {
                    Desktop::Gnome => "gnome-night-light.txt",
                    Desktop::Kde => "kde-night-light.txt",
                };
                traykit::dirs::config_dir().map(|d| d.join(file))
            }
        }

        fn kde_type(key: &str) -> &'static str {
            match key {
                "Active" => "bool",
                "NightTemperature" => "int",
                _ => "string",
            }
        }

        /// Save the user's settings, unless a run that didn't get to quit
        /// already did: what's there now is Luz's, not theirs.
        fn save_theirs(d: Desktop) {
            let Some(path) = d.saved_path() else { return };
            if path.exists() {
                return;
            }
            let lines: Vec<String> = d
                .keys()
                .iter()
                .filter_map(|k| d.get(k).map(|v| format!("{k} {v}").trim_end().to_string()))
                .collect();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(&path, lines.join("\n") + "\n") {
                lwarn!("could not save Night Light settings: {e}");
            }
        }

        /// The temperature to ask for, or `None` for Night Light off.
        pub(in super::super) fn wanted(kelvin: f64, (lo, hi): (u32, u32)) -> Option<u32> {
            (kelvin < OFF_ABOVE).then(|| ((kelvin / 50.0).round() as u32 * 50).clamp(lo, hi))
        }

        pub fn set(d: Desktop, kelvin: f64) -> super::Outcome {
            let want = wanted(kelvin, d.range());
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            let ok = if *last == Some(want) {
                true
            } else {
                if last.is_none() {
                    save_theirs(d);
                }
                let ok = match want {
                    // Mid-fade, only the temperature moves.
                    Some(k) if matches!(*last, Some(Some(_))) => d.temperature(k),
                    Some(k) => d.on(k),
                    None => d.off(),
                };
                *last = ok.then_some(want);
                ok
            };
            super::Outcome {
                screens: usize::from(ok),
                refused: 0,
            }
        }

        /// The user's own settings back, from the saved file. A key saved
        /// with no value wasn't set, and goes back to the default.
        pub fn restore(d: Desktop) {
            *LAST.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let Some(path) = d.saved_path() else { return };
            let Ok(text) = std::fs::read_to_string(&path) else {
                return;
            };
            for line in text.lines() {
                let (key, value) = line.split_once(' ').unwrap_or((line, ""));
                if d.keys().contains(&key) {
                    d.put(key, value);
                }
            }
            let _ = std::fs::remove_file(path);
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn night_light_rounds_and_switches_off_for_daylight() {
            use super::night_light::wanted;
            let gnome = (1000, 10000);
            assert_eq!(wanted(3412.0, gnome), Some(3400));
            assert_eq!(wanted(1200.0, gnome), Some(1200));
            assert_eq!(wanted(6500.0, gnome), None);
            assert_eq!(wanted(6470.0, gnome), None);
            assert_eq!(wanted(6420.0, (1000, 6400)), Some(6400));
        }
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
            let out = set(k as f64, crate::color::gains(k as f64), 1.0);
            eprintln!("{k} K: {out:?}");
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        restore();
        eprintln!("unlocked: {}", warmer_unlocked());
    }
}
