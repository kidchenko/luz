//! Small helpers shared across modules.

/// Seconds since the Unix epoch, saturating at 0 on a pre-1970 clock.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The local date and time, from the OS clock and its idea of the zone and
/// DST, so no date crate is needed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Minutes since local midnight, with seconds as the fraction.
    pub minute: f64,
    /// How far the local clock is ahead of UTC, DST included.
    pub utc_offset: i32,
}

#[cfg(windows)]
pub fn local_now() -> Local {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    let (local, utc) = unsafe { (GetLocalTime(), GetSystemTime()) };
    let minutes = |t: &SYSTEMTIME| {
        days_from_civil(t.wYear as i32, t.wMonth as u32, t.wDay as u32) * 1440
            + t.wHour as i64 * 60
            + t.wMinute as i64
    };
    Local {
        year: local.wYear as i32,
        month: local.wMonth as u32,
        day: local.wDay as u32,
        minute: local.wHour as f64 * 60.0
            + local.wMinute as f64
            + (local.wSecond as f64 + local.wMilliseconds as f64 / 1000.0) / 60.0,
        utc_offset: (minutes(&local) - minutes(&utc)) as i32,
    }
}

#[cfg(unix)]
pub fn local_now() -> Local {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    Local {
        year: tm.tm_year + 1900,
        month: tm.tm_mon as u32 + 1,
        day: tm.tm_mday as u32,
        minute: tm.tm_hour as f64 * 60.0
            + tm.tm_min as f64
            + (tm.tm_sec as f64 + dur.subsec_millis() as f64 / 1000.0) / 60.0,
        utc_offset: (tm.tm_gmtoff / 60) as i32,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
#[cfg(any(windows, test))]
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64 - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "HH:MM" for a time `minutes` after local midnight, wrapping past a day.
pub fn clock_label(minutes: f64) -> String {
    let m = (minutes.round() as i64).rem_euclid(1440);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// "07:30" to minutes after midnight.
pub fn parse_clock(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// "45 minutes", "1 hour", "1 h 30 min", for how long a pause has left.
pub fn span(secs: u64) -> String {
    let mins = secs.div_ceil(60);
    match mins {
        0 | 1 => "1 minute".into(),
        m if m < 60 => format!("{m} minutes"),
        60 => "1 hour".into(),
        m if m % 60 == 0 => format!("{} hours", m / 60),
        m => format!("{} h {:02} min", m / 60, m % 60),
    }
}

/// Full path to the running executable, quoted for the Run key.
#[cfg(windows)]
pub fn exe_path_quoted() -> Option<String> {
    let p = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", p.display()))
}

/// Borrow the parent terminal's console, so `luz status` and friends can
/// print. A `windows_subsystem = "windows"` binary starts without one. Harmless
/// when launched from Explorer, where there is no parent console to borrow.
#[cfg(windows)]
pub fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use windows::core::PCWSTR;

    /// Null-terminated UTF-16. Keep the buffer alive while the PCWSTR is used.
    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn pcwstr(buf: &[u16]) -> PCWSTR {
        PCWSTR(buf.as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks() {
        assert_eq!(clock_label(0.0), "00:00");
        assert_eq!(clock_label(7.0 * 60.0 + 29.6), "07:30");
        assert_eq!(clock_label(1440.0 + 61.0), "01:01");
        assert_eq!(clock_label(-30.0), "23:30");
        assert_eq!(parse_clock("07:30"), Some(450));
        assert_eq!(parse_clock(" 23:05 "), Some(1385));
        assert_eq!(parse_clock("24:00"), None);
        assert_eq!(parse_clock("7"), None);
    }

    #[test]
    fn spans() {
        assert_eq!(span(30), "1 minute");
        assert_eq!(span(45 * 60), "45 minutes");
        assert_eq!(span(3600), "1 hour");
        assert_eq!(span(5400), "1 h 30 min");
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(
            days_from_civil(2026, 10, 23) - days_from_civil(2026, 10, 22),
            1
        );
    }

    #[test]
    fn the_clock_reads_sensibly() {
        let now = local_now();
        assert!((1..=12).contains(&now.month));
        assert!((0.0..1440.0).contains(&now.minute));
        assert!(now.utc_offset.abs() <= 14 * 60);
    }
}
