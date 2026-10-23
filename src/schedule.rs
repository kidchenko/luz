//! What colour the screen should be at a given minute of the day.
//!
//! Two fades, each over `fade` minutes, that overlap where the day says so:
//!
//! * **daylight**, from the sunset colour up to the daytime one in the
//!   morning and back down at sunset. Mornings start at sunrise or, with
//!   Bedtime mode on, at your wake time if that is later.
//! * **bedtime**, from whatever the sun says down to the bedtime colour, at
//!   bedtime, and back up when you wake.
//!
//! Fades start at their moment rather than finishing at it, as f.lux's do:
//! the screen starts warming at sunset and is fully warm `fade` later.

use crate::color::mix;
use crate::sun::Day;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Day,
    /// After sunset, before bedtime.
    Evening,
    Bedtime,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Sunrise,
    /// Morning held back to your wake time.
    Wake,
    Sunset,
    Bedtime,
}

impl Event {
    pub fn label(self) -> &'static str {
        match self {
            Event::Sunrise => "Sunrise",
            Event::Wake => "Morning",
            Event::Sunset => "Sunset",
            Event::Bedtime => "Bedtime",
        }
    }
}

/// The settings that shape the day.
#[derive(Clone, Copy, Debug)]
pub struct Colours {
    pub day: f64,
    pub sunset: f64,
    pub bedtime: f64,
    /// Bedtime mode's window, in minutes after midnight; `None` when it's off.
    pub bed: Option<(f64, f64)>,
    /// Minutes each fade takes.
    pub fade: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub kelvin: f64,
    pub phase: Phase,
    /// Part way through a fade.
    pub fading: bool,
    /// The next change of phase, and when (minutes after midnight, possibly
    /// tomorrow's: always within a day of `minute`).
    pub next: Option<(Event, f64)>,
}

/// How far along a fade that began `since` minutes ago is.
fn ramp(since: f64, fade: f64) -> f64 {
    if fade <= 0.0 {
        f64::from(since >= 0.0)
    } else {
        (since / fade).clamp(0.0, 1.0)
    }
}

/// Minutes from `from` forward to `to`, round the clock.
fn ahead(from: f64, to: f64) -> f64 {
    (to - from).rem_euclid(1440.0)
}

/// When the morning fade starts, or `None` with no sunrise today.
fn morning(day: Day, c: &Colours) -> Option<(Event, f64)> {
    let Day::Normal { rise, set } = day else {
        return None;
    };
    match c.bed {
        Some((_, wake)) if wake > rise && wake < set => Some((Event::Wake, wake)),
        _ => Some((Event::Sunrise, rise)),
    }
}

/// 0 at night, 1 in the day, in between while fading.
fn daylight(minute: f64, day: Day, c: &Colours) -> f64 {
    match day {
        Day::AlwaysUp => 1.0,
        Day::AlwaysDown => 0.0,
        Day::Normal { set, .. } => {
            let (_, start) = morning(day, c).unwrap_or((Event::Sunrise, 0.0));
            if minute < start {
                0.0
            } else {
                ramp(minute - start, c.fade) * (1.0 - ramp(minute - set, c.fade))
            }
        }
    }
}

/// 0 outside Bedtime mode's window, 1 inside it, in between while fading.
fn bedtime(minute: f64, c: &Colours) -> f64 {
    let Some((bed, wake)) = c.bed else {
        return 0.0;
    };
    let length = ahead(bed, wake);
    let since_bed = ahead(bed, minute);
    if since_bed < length {
        ramp(since_bed, c.fade)
    } else {
        1.0 - ramp(ahead(wake, minute), c.fade)
    }
}

fn phase_of(daylight: f64, bedtime: f64) -> Phase {
    if bedtime >= 0.5 {
        Phase::Bedtime
    } else if daylight >= 0.5 {
        Phase::Day
    } else {
        Phase::Evening
    }
}

fn at(minute: f64, day: Day, c: &Colours) -> (f64, Phase, bool) {
    let d = daylight(minute, day, c);
    let b = bedtime(minute, c);
    let kelvin = mix(mix(c.sunset, c.day, d), c.bedtime, b);
    let fading = (d > 0.0 && d < 1.0) || (b > 0.0 && b < 1.0);
    (kelvin, phase_of(d, b), fading)
}

/// The plan for `minute` (after local midnight) on a day with `day`'s sun.
pub fn plan(minute: f64, day: Day, c: &Colours) -> Plan {
    let (kelvin, phase, fading) = at(minute, day, c);

    let mut events = Vec::new();
    if let Some(m) = morning(day, c) {
        events.push(m);
    }
    if let Day::Normal { set, .. } = day {
        events.push((Event::Sunset, set));
    }
    if let Some((bed, _)) = c.bed {
        events.push((Event::Bedtime, bed));
    }
    // The soonest one that changes what the screen is doing. Looked at once
    // its fade is over, so an event that changes nothing (sunset after
    // bedtime has begun) is skipped.
    events.sort_by(|a, b| ahead(minute, a.1).total_cmp(&ahead(minute, b.1)));
    let next = events.into_iter().find(|&(_, m)| {
        let done = (m + c.fade.max(1.0) + 0.5).rem_euclid(1440.0);
        ahead(minute, m) > 0.0 && at(done, day, c).1 != phase
    });
    let next = next.map(|(e, m)| (e, m.rem_euclid(1440.0)));

    Plan {
        kelvin,
        phase,
        fading,
        next,
    }
}

/// The next morning, as minutes from `minute` and the clock time it starts,
/// for "Pause until sunrise". `None` where the sun doesn't rise today.
pub fn next_morning(minute: f64, day: Day, c: &Colours) -> Option<(Event, f64, f64)> {
    let (event, at) = morning(day, c)?;
    let wait = ahead(minute, at);
    Some((event, if wait > 0.0 { wait } else { 1440.0 }, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mornings_come_round() {
        let (e, wait, at) = next_morning(20.0 * 60.0, SAO_PAULO, &c(false)).unwrap();
        assert_eq!((e, at), (Event::Sunrise, 5.0 * 60.0 + 35.0));
        assert_eq!(wait, 9.0 * 60.0 + 35.0);
        assert_eq!(next_morning(0.0, Day::AlwaysDown, &c(false)), None);
    }

    const SAO_PAULO: Day = Day::Normal {
        rise: 5.0 * 60.0 + 35.0,
        set: 18.0 * 60.0 + 15.0,
    };

    fn c(bed: bool) -> Colours {
        Colours {
            day: 6500.0,
            sunset: 3400.0,
            bedtime: 1900.0,
            bed: bed.then_some((23.0 * 60.0, 7.0 * 60.0)),
            fade: 60.0,
        }
    }

    fn k(minute: f64, bed: bool) -> f64 {
        plan(minute, SAO_PAULO, &c(bed)).kelvin.round()
    }

    #[test]
    fn a_day_without_bedtime_mode() {
        assert_eq!(k(3.0 * 60.0, false), 3400.0);
        assert_eq!(
            k(5.0 * 60.0 + 35.0, false),
            3400.0,
            "fades start at sunrise"
        );
        assert_eq!(k(7.0 * 60.0, false), 6500.0);
        assert_eq!(k(18.0 * 60.0 + 15.0, false), 6500.0);
        let dusk = k(18.0 * 60.0 + 45.0, false);
        assert!(dusk > 3400.0 && dusk < 6500.0, "{dusk}");
        assert_eq!(k(19.0 * 60.0 + 15.0, false), 3400.0);
    }

    #[test]
    fn bedtime_mode_runs_over_midnight_and_holds_the_morning() {
        assert_eq!(k(22.0 * 60.0, true), 3400.0);
        assert_eq!(k(0.0, true), 1900.0);
        assert_eq!(k(6.0 * 60.0, true), 1900.0, "the sun is up, you aren't");
        // At 7:00 bedtime lifts and the day starts together.
        assert_eq!(k(8.0 * 60.0 + 1.0, true), 6500.0);
        let p = plan(6.0 * 60.0, SAO_PAULO, &c(true));
        assert_eq!(p.phase, Phase::Bedtime);
        assert_eq!(p.next, Some((Event::Wake, 7.0 * 60.0)));
    }

    #[test]
    fn says_what_comes_next() {
        let next = |m: f64| plan(m, SAO_PAULO, &c(true)).next.unwrap();
        assert_eq!(next(12.0 * 60.0), (Event::Sunset, 18.0 * 60.0 + 15.0));
        assert_eq!(next(20.0 * 60.0), (Event::Bedtime, 23.0 * 60.0));
        let quiet = plan(12.0 * 60.0, SAO_PAULO, &c(false)).next.unwrap();
        assert_eq!(quiet.0, Event::Sunset);
        assert_eq!(
            plan(20.0 * 60.0, SAO_PAULO, &c(false)).next.unwrap(),
            (Event::Sunrise, 5.0 * 60.0 + 35.0)
        );
    }

    #[test]
    fn a_bedtime_before_sunset_skips_the_evening() {
        // A northern summer: sunset at 22:30, bed at 22:00.
        let late = Day::Normal {
            rise: 4.0 * 60.0,
            set: 22.0 * 60.0 + 30.0,
        };
        let mut early = c(true);
        early.bed = Some((22.0 * 60.0, 7.0 * 60.0));
        let p = plan(21.0 * 60.0, late, &early);
        assert_eq!(p.next.unwrap().0, Event::Bedtime);
        assert_eq!(plan(23.5 * 60.0, late, &early).kelvin.round(), 1900.0);
    }

    #[test]
    fn polar_days() {
        assert_eq!(plan(1.0, Day::AlwaysUp, &c(false)).kelvin, 6500.0);
        assert_eq!(plan(720.0, Day::AlwaysDown, &c(false)).kelvin, 3400.0);
        assert_eq!(plan(720.0, Day::AlwaysDown, &c(false)).next, None);
    }

    #[test]
    fn fast_fades() {
        let mut fast = c(false);
        fast.fade = 20.0 / 60.0;
        let set = 18.0 * 60.0 + 15.0;
        assert_eq!(plan(set + 1.0, SAO_PAULO, &fast).kelvin.round(), 3400.0);
        assert!(plan(set + 0.1, SAO_PAULO, &fast).fading);
    }
}
