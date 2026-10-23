//! The thread that owns the screens' ramps.
//!
//! The app says where the colour should go and how quickly; this thread gets
//! it there at 30 frames a second, so a change never shows as a jump, and
//! sets the ramps again every few seconds while nothing is changing, because
//! Windows drops them after sleep, a resolution change or a full-screen game.
//!
//! Where Windows refuses a ramp (see `gamma::unlock_warmer`), it backs off
//! toward plain until one is taken, and says so, so the menu can explain why
//! the screen isn't as warm as asked.

use crate::color::{self, NEUTRAL_K};
use crate::gamma;
use crate::log::{linfo, lwarn};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_millis(33);
/// How often to set unchanged ramps again.
const REASSERT: Duration = Duration::from_secs(3);
/// How much nearer plain each refusal steps.
const BACKOFF_STEP: f64 = 0.1;

/// What is on the screens now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub kelvin: f64,
    pub brightness: f64,
    /// Windows wouldn't take the colour asked for, so this is milder.
    pub limited: bool,
    /// Screens tinted; 0 means none would take a ramp at all.
    pub screens: usize,
}

impl Default for Shown {
    /// Plain, on however many screens there are.
    fn default() -> Self {
        Self {
            kelvin: NEUTRAL_K as f64,
            brightness: 1.0,
            limited: false,
            screens: 1,
        }
    }
}

enum Msg {
    To {
        kelvin: f64,
        brightness: f64,
        over: Duration,
    },
    Quit,
}

pub struct Screen {
    tx: Sender<Msg>,
    shown: Arc<Mutex<Shown>>,
    thread: Option<JoinHandle<()>>,
}

impl Screen {
    /// Start at `kelvin` straight away: at launch the screen was plain, and
    /// fading in from it would only draw the eye.
    pub fn start(kelvin: f64, brightness: f64) -> Self {
        let (tx, rx) = mpsc::channel();
        let shown = Arc::new(Mutex::new(Shown::default()));
        // The first ramps go on here, so the screens are right before the
        // tray icon appears and `shown` is real from the start.
        let mut w = Worker::new(kelvin, brightness, shown.clone());
        w.step();
        let thread = std::thread::Builder::new()
            .name("luz-screen".into())
            .spawn(move || {
                loop {
                    let wait = if w.moving() {
                        FRAME
                    } else {
                        REASSERT.saturating_sub(w.applied_at.elapsed())
                    };
                    match rx.recv_timeout(wait) {
                        Ok(Msg::To {
                            kelvin,
                            brightness,
                            over,
                        }) => w.head_for(kelvin, brightness, over),
                        Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    w.step();
                }
                gamma::restore();
                linfo!("screens put back");
            })
            .ok();
        Self { tx, shown, thread }
    }

    /// Fade to `kelvin` at `brightness` over `over`.
    pub fn go(&self, kelvin: f64, brightness: f64, over: Duration) {
        let _ = self.tx.send(Msg::To {
            kelvin,
            brightness,
            over,
        });
    }

    pub fn shown(&self) -> Shown {
        *self.shown.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for Screen {
    /// Put the screens back before the process goes.
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Worker {
    from: (f64, f64),
    to: (f64, f64),
    now: (f64, f64),
    started: Instant,
    over: Duration,
    applied: Option<(f64, f64)>,
    applied_at: Instant,
    /// How far toward plain Windows has made us back off, 0 to 1.
    backoff: f64,
    shown: Arc<Mutex<Shown>>,
}

impl Worker {
    fn new(kelvin: f64, brightness: f64, shown: Arc<Mutex<Shown>>) -> Self {
        let at = (kelvin, brightness);
        Self {
            from: at,
            to: at,
            now: at,
            started: Instant::now(),
            over: Duration::ZERO,
            applied: None,
            applied_at: Instant::now(),
            backoff: 0.0,
            shown,
        }
    }

    fn moving(&self) -> bool {
        self.started.elapsed() < self.over
    }

    fn head_for(&mut self, kelvin: f64, brightness: f64, over: Duration) {
        self.from = self.now;
        self.to = (kelvin, brightness);
        self.started = Instant::now();
        self.over = over;
    }

    fn step(&mut self) {
        let t = if self.over.is_zero() {
            1.0
        } else {
            (self.started.elapsed().as_secs_f64() / self.over.as_secs_f64()).min(1.0)
        };
        // Eased at both ends, as a dimmer is turned by hand.
        let e = t * t * (3.0 - 2.0 * t);
        self.now = (
            color::mix(self.from.0, self.to.0, e),
            self.from.1 + (self.to.1 - self.from.1) * e,
        );
        let changed = self.applied.map_or(true, |(k, b)| {
            (1e6 / k - 1e6 / self.now.0).abs() > 0.05 || (b - self.now.1).abs() > 0.0005
        });
        if changed || self.applied_at.elapsed() >= REASSERT {
            self.apply();
        }
    }

    /// Set the ramps, stepping toward plain while Windows refuses them. Once
    /// backed off, try the colour in full each time the ramps are set again,
    /// then a step bolder: a refusal can be a passing one (a screen waking
    /// up, the lock screen), and the backoff should lift as soon as it's over
    /// or the colour asked for gets milder.
    fn apply(&mut self) {
        let (kelvin, brightness) = self.now;
        let gains = color::gains(kelvin);
        let try_at = |t: f64| {
            let g = gains.map(|v| v + (1.0 - v) * t);
            gamma::set(g, brightness + (1.0 - brightness) * t)
        };
        let reassert = self.applied_at.elapsed() >= REASSERT;
        if self.backoff > 0.0 && reassert {
            for bolder in [0.0, (self.backoff - BACKOFF_STEP).max(0.0)] {
                if try_at(bolder).refused == 0 {
                    self.backoff = bolder;
                    break;
                }
            }
        }
        let mut out = try_at(self.backoff);
        while out.refused > 0 && self.backoff < 1.0 {
            self.backoff = (self.backoff + BACKOFF_STEP).min(1.0);
            out = try_at(self.backoff);
        }
        if out.refused > 0 {
            lwarn!("no screen would take a gamma ramp");
        }
        self.applied = Some(self.now);
        self.applied_at = Instant::now();

        let shown = Shown {
            kelvin: color::mix(kelvin, NEUTRAL_K as f64, self.backoff),
            brightness: brightness + (1.0 - brightness) * self.backoff,
            limited: self.backoff > 0.0,
            screens: out.screens,
        };
        let mut s = self.shown.lock().unwrap_or_else(|e| e.into_inner());
        if s.limited != shown.limited || s.screens != shown.screens {
            linfo!(
                "{} screen(s) tinted{}",
                shown.screens,
                if shown.limited {
                    format!(", held at about {:.0} K by Windows", shown.kelvin)
                } else {
                    String::new()
                }
            );
        }
        *s = shown;
    }
}

#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore = "changes the screen's colour for a moment"]
    fn backs_off_where_windows_says_no() {
        let s = Screen::start(1900.0, 1.0);
        let shown = s.shown();
        eprintln!("asked 1900 K, shown {shown:?}");
        assert!(shown.screens > 0);
        std::thread::sleep(Duration::from_millis(500));
    }
}
