//! Monitor backlights, over DDC/CI: the channel a monitor's own buttons
//! answer to, so dimming here really turns the light down, rather than
//! darkening the picture the way the gamma ramp does.
//!
//! Only monitors that answer show up. Laptop panels don't speak DDC/CI (their
//! brightness keys already do this), and some docks, adapters and TVs don't
//! pass it through. With none that answer, Luz dims with the ramp instead.
//!
//! A DDC/CI call takes tens of milliseconds, sometimes more, so they all run
//! on a thread of their own and the menu reads the last known levels.

use crate::log::linfo;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often to read the levels again, to notice a monitor's own buttons or
/// a monitor plugged in.
const REREAD: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub name: String,
    /// 0 to 100.
    pub level: u8,
}

enum Cmd {
    /// Every monitor up or down by this many points.
    Step(i32),
    /// One monitor, by its place in the list, to a level.
    Set(usize, u8),
}

pub struct Backlight {
    tx: Sender<Cmd>,
    known: Arc<Mutex<Vec<Monitor>>>,
}

impl Backlight {
    /// `changed` runs on the backlight thread whenever the levels or the
    /// monitors change, so the menu can catch up.
    pub fn start(changed: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let known = Arc::new(Mutex::new(Vec::new()));
        let out = known.clone();
        let _ = std::thread::Builder::new()
            .name("luz-backlight".into())
            .spawn(move || {
                let publish = |fresh: Vec<Monitor>| {
                    let mut k = out.lock().unwrap_or_else(|e| e.into_inner());
                    if *k != fresh {
                        if k.len() != fresh.len() {
                            let names: Vec<_> = fresh.iter().map(|m| m.name.as_str()).collect();
                            linfo!("backlights: {}", names.join(", "));
                        }
                        *k = fresh;
                        drop(k);
                        changed();
                    }
                };
                publish(imp::read());
                loop {
                    match rx.recv_timeout(REREAD) {
                        Ok(Cmd::Step(by)) => imp::step(by),
                        Ok(Cmd::Set(i, level)) => imp::set(i, level),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    publish(imp::read());
                }
            });
        Self { tx, known }
    }

    /// The monitors that answer, as last read.
    pub fn monitors(&self) -> Vec<Monitor> {
        self.known.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn step(&self, by: i32) {
        let _ = self.tx.send(Cmd::Step(by));
    }

    pub fn set(&self, index: usize, level: u8) {
        let _ = self.tx.send(Cmd::Set(index, level));
    }
}

/// A level on a monitor's own scale, as a percentage, and back.
#[cfg(any(windows, test))]
fn to_percent(min: u32, cur: u32, max: u32) -> u8 {
    if max <= min {
        return 100;
    }
    ((cur.clamp(min, max) - min) as f64 * 100.0 / (max - min) as f64).round() as u8
}

#[cfg(any(windows, test))]
fn from_percent(min: u32, percent: i32, max: u32) -> u32 {
    let p = percent.clamp(0, 100) as f64 / 100.0;
    min + ((max.saturating_sub(min)) as f64 * p).round() as u32
}

/// "Generic PnP Monitor" says nothing; number those instead.
#[cfg(any(windows, test))]
fn name(description: &str, n: usize) -> String {
    let d = description.trim();
    if d.is_empty() || d.to_ascii_lowercase().contains("generic") {
        format!("Monitor {n}")
    } else {
        d.to_string()
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod imp {
    use super::{from_percent, name, to_percent, Monitor};
    use crate::log::lwarn;
    use windows::core::BOOL;
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

    unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        let out = &mut *(out.0 as *mut Vec<HMONITOR>);
        out.push(m);
        true.into()
    }

    /// Every monitor that answers DDC/CI, with its level as (min, cur, max).
    /// The handles are only good inside `f`.
    fn each(mut f: impl FnMut(usize, HANDLE, &str, (u32, u32, u32))) {
        let mut screens: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(collect),
                LPARAM(&mut screens as *mut _ as isize),
            );
        }
        let mut index = 0;
        for screen in screens {
            let mut n = 0u32;
            if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(screen, &mut n) }.is_err() || n == 0
            {
                continue;
            }
            let mut physical = vec![PHYSICAL_MONITOR::default(); n as usize];
            if unsafe { GetPhysicalMonitorsFromHMONITOR(screen, &mut physical) }.is_err() {
                continue;
            }
            for p in &physical {
                // PHYSICAL_MONITOR is packed: copy its fields out, never
                // borrow them.
                let (h, d) = (p.hPhysicalMonitor, p.szPhysicalMonitorDescription);
                let (mut min, mut cur, mut max) = (0, 0, 0);
                if unsafe { GetMonitorBrightness(h, &mut min, &mut cur, &mut max) } == 0 {
                    continue;
                }
                let len = d.iter().position(|&c| c == 0).unwrap_or(d.len());
                f(
                    index,
                    h,
                    &String::from_utf16_lossy(&d[..len]),
                    (min, cur, max),
                );
                index += 1;
            }
            let _ = unsafe { DestroyPhysicalMonitors(&physical) };
        }
    }

    pub fn read() -> Vec<Monitor> {
        let mut out = Vec::new();
        each(|i, _, desc, (min, cur, max)| {
            out.push(Monitor {
                name: name(desc, i + 1),
                level: to_percent(min, cur, max),
            })
        });
        out
    }

    fn write(h: HANDLE, desc: &str, (min, _, max): (u32, u32, u32), percent: i32) {
        if unsafe { SetMonitorBrightness(h, from_percent(min, percent, max)) } == 0 {
            lwarn!("{desc} didn't take a brightness of {percent}%");
        }
    }

    pub fn step(by: i32) {
        each(|_, h, desc, level| {
            let now = to_percent(level.0, level.1, level.2) as i32;
            // Back onto the step grid, so 47% goes to 40% or 50%.
            let target = ((now + by) as f64 / by.abs() as f64).round() as i32 * by.abs();
            write(h, desc, level, target);
        });
    }

    pub fn set(index: usize, percent: u8) {
        each(|i, h, desc, level| {
            if i == index {
                write(h, desc, level, percent as i32);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------
// DDC/CI on a Mac goes through private IOKit services (IOAVService on Apple
// silicon), as MonitorControl and Lunar do. Not here yet: with no monitors
// listed, Luz dims with the gamma ramp instead.
//
// Linux: DDC/CI goes through /dev/i2c-*, which needs a group most users
// aren't in. Not here either.
#[cfg(not(windows))]
mod imp {
    use super::Monitor;

    pub fn read() -> Vec<Monitor> {
        Vec::new()
    }

    pub fn step(_by: i32) {}

    pub fn set(_index: usize, _percent: u8) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_on_any_scale() {
        assert_eq!(to_percent(0, 50, 100), 50);
        assert_eq!(to_percent(10, 10, 60), 0);
        assert_eq!(to_percent(0, 300, 100), 100);
        assert_eq!(to_percent(5, 5, 5), 100);
        assert_eq!(from_percent(0, 40, 100), 40);
        assert_eq!(from_percent(10, 50, 60), 35);
        assert_eq!(from_percent(0, 140, 100), 100);
    }

    #[test]
    fn generic_monitors_get_numbers() {
        assert_eq!(name("Generic PnP Monitor", 2), "Monitor 2");
        assert_eq!(name("DELL U2720Q", 1), "DELL U2720Q");
    }

    /// Reads only; changes nothing.
    #[test]
    #[ignore = "talks to the monitors"]
    fn what_answers_here() {
        eprintln!("{:?}", imp::read());
    }
}

#[cfg(all(test, windows))]
mod live {
    use super::*;

    #[test]
    #[ignore = "changes the monitors' brightness for a moment"]
    fn steps_down_and_back() {
        let before = imp::read();
        imp::step(-10);
        let lower = imp::read();
        imp::step(10);
        let after = imp::read();
        eprintln!("{before:?} -> {lower:?} -> {after:?}");
        assert_eq!(before, after);
    }
}
