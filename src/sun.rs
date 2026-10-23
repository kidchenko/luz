//! When the sun rises and sets, from NOAA's sunrise equation: the equation of
//! time and the sun's declination for the day of the year, then the hour angle
//! at which its upper edge meets the horizon, refraction included (90.833°).
//! Good to a minute or two, which is far finer than a fade needs.

/// Sunrise and sunset on one day, in minutes after local midnight. Either
/// can fall outside 0..1440 when a zone sits far from its meridian.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Day {
    Normal {
        rise: f64,
        set: f64,
    },
    /// The sun stays up (summer beyond the polar circles).
    AlwaysUp,
    /// The sun stays down.
    AlwaysDown,
}

/// The sun's day at `lat`, `lon` (degrees, east positive) on day `doy` of the
/// year (1 = 1 January), for a clock `utc_offset` minutes ahead of UTC.
pub fn day(lat: f64, lon: f64, doy: u32, utc_offset: i32) -> Day {
    // The fractional year at local noon, in radians.
    let g = 2.0 * std::f64::consts::PI / 365.0 * (doy as f64 - 1.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let phi = lat.clamp(-89.9, 89.9).to_radians();
    let cos_ha = 90.833_f64.to_radians().cos() / (phi.cos() * decl.cos()) - phi.tan() * decl.tan();
    if cos_ha > 1.0 {
        return Day::AlwaysDown;
    }
    if cos_ha < -1.0 {
        return Day::AlwaysUp;
    }
    let ha = cos_ha.acos().to_degrees();
    let offset = utc_offset as f64;
    Day::Normal {
        rise: 720.0 - 4.0 * (lon + ha) - eqtime + offset,
        set: 720.0 - 4.0 * (lon - ha) - eqtime + offset,
    }
}

/// Day of the year, 1 for 1 January.
pub fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    const BEFORE: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let m = month.clamp(1, 12) as usize;
    BEFORE[m - 1] + day + u32::from(leap && m > 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(minutes: f64) -> String {
        let m = minutes.round() as i64;
        format!("{:02}:{:02}", m / 60, m % 60)
    }

    fn times(lat: f64, lon: f64, (y, m, d): (i32, u32, u32), offset: i32) -> (String, String) {
        match day(lat, lon, day_of_year(y, m, d), offset) {
            Day::Normal { rise, set } => (hm(rise), hm(set)),
            other => panic!("{other:?}"),
        }
    }

    /// Within `slack` minutes of the almanac.
    fn about(got: &str, want: &str, slack: i64) {
        let mins = |s: &str| {
            let (h, m) = s.split_once(':').unwrap();
            h.parse::<i64>().unwrap() * 60 + m.parse::<i64>().unwrap()
        };
        assert!(
            (mins(got) - mins(want)).abs() <= slack,
            "got {got}, want {want}"
        );
    }

    #[test]
    fn matches_the_almanac() {
        // NOAA's full solar calculator, rounded to the minute.
        let (r, s) = times(51.51, -0.13, (2026, 6, 21), 60);
        about(&r, "04:43", 3);
        about(&s, "21:22", 3);
        let (r, s) = times(-23.55, -46.63, (2026, 10, 23), -180);
        about(&r, "05:27", 3);
        about(&s, "18:15", 3);
        let (r, s) = times(40.71, -74.01, (2026, 12, 21), -300);
        about(&r, "07:17", 3);
        about(&s, "16:32", 3);
    }

    #[test]
    fn polar_days_and_nights() {
        let june = day_of_year(2026, 6, 21);
        assert_eq!(day(78.2, 15.6, june, 120), Day::AlwaysUp);
        assert_eq!(day(-78.2, 15.6, june, 120), Day::AlwaysDown);
    }

    #[test]
    fn days_of_the_year() {
        assert_eq!(day_of_year(2026, 1, 1), 1);
        assert_eq!(day_of_year(2026, 12, 31), 365);
        assert_eq!(day_of_year(2028, 3, 1), 61);
    }
}
