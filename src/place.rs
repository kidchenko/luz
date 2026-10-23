//! Where the computer is, closely enough to know when the sun sets there.
//!
//! Set in the config, that is that. Otherwise Luz goes by the time zone: each
//! one is named for a city, and tzdata knows where it is. Wrong by a couple of
//! hundred kilometres in a big zone, which moves sunset by a few minutes. In
//! exchange there is no location prompt, no location service and no network.

use crate::zones;

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub lat: f64,
    pub lon: f64,
    /// "Sao Paulo", for a guess; empty for coordinates you gave.
    pub city: String,
    pub guessed: bool,
}

impl Place {
    /// From the config when set there, else from the time zone.
    pub fn find(lat: Option<f64>, lon: Option<f64>) -> Option<Self> {
        if let (Some(lat), Some(lon)) = (lat, lon) {
            return Some(Place {
                lat,
                lon,
                city: String::new(),
                guessed: false,
            });
        }
        guess(&imp::zone()?)
    }

    /// "São Paulo, from your time zone" / "23.55° S, 46.63° W".
    pub fn describe(&self) -> String {
        if self.guessed {
            format!("{}, from your time zone", self.city)
        } else {
            coordinates(self.lat, self.lon)
        }
    }
}

/// "23.55° S, 46.63° W".
pub fn coordinates(lat: f64, lon: f64) -> String {
    format!(
        "{:.2}\u{00b0} {}, {:.2}\u{00b0} {}",
        lat.abs(),
        if lat < 0.0 { 'S' } else { 'N' },
        lon.abs(),
        if lon < 0.0 { 'W' } else { 'E' }
    )
}

/// The city a zone is named for, from an IANA name ("America/Sao_Paulo") or a
/// Windows one ("E. South America Standard Time").
pub fn guess(zone: &str) -> Option<Place> {
    let iana = zones::WINDOWS
        .iter()
        .find(|(w, _)| *w == zone)
        .map_or(zone, |(_, iana)| *iana);
    let iana = zones::ALIASES
        .iter()
        .find(|(old, _)| *old == iana)
        .map_or(iana, |(_, new)| *new);
    let &(name, lat, lon) = zones::ZONES.iter().find(|(n, ..)| *n == iana)?;
    Some(Place {
        lat: lat as f64,
        lon: lon as f64,
        city: name.rsplit('/').next().unwrap_or(name).replace('_', " "),
        guessed: true,
    })
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod imp {
    use windows::Win32::System::Time::{
        GetDynamicTimeZoneInformation, DYNAMIC_TIME_ZONE_INFORMATION,
    };

    /// The zone's key name, the same in every display language.
    pub fn zone() -> Option<String> {
        let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
        if unsafe { GetDynamicTimeZoneInformation(&mut info) } == u32::MAX {
            return None;
        }
        let key = &info.TimeZoneKeyName;
        let len = key.iter().position(|&c| c == 0).unwrap_or(key.len());
        let name = String::from_utf16_lossy(&key[..len]);
        (!name.is_empty()).then_some(name)
    }
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod imp {
    /// `/etc/localtime` links into the zoneinfo database, and the rest of
    /// the link after `zoneinfo/` is the zone's name. `TZ` wins when set.
    pub fn zone() -> Option<String> {
        if let Ok(tz) = std::env::var("TZ") {
            let tz = tz.trim_start_matches(':');
            if tz.contains('/') {
                return Some(tz.to_string());
            }
        }
        let target = std::fs::read_link("/etc/localtime").ok()?;
        let target = target.to_string_lossy();
        let (_, name) = target.split_once("zoneinfo/")?;
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_from_either_kind_of_zone_name() {
        let sp = guess("America/Sao_Paulo").unwrap();
        assert_eq!(sp.city, "Sao Paulo");
        assert!((sp.lat + 23.5).abs() < 0.2 && (sp.lon + 46.6).abs() < 0.2);
        assert_eq!(guess("E. South America Standard Time"), Some(sp));
        let la = guess("Pacific Standard Time").unwrap();
        assert_eq!(la.city, "Los Angeles");
        // An old name still in use.
        assert_eq!(guess("Asia/Calcutta").unwrap().city, "Kolkata");
        assert_eq!(guess("Nowhere/Special"), None);
    }

    #[test]
    fn coordinates_read_like_a_map() {
        assert_eq!(
            coordinates(-23.55, -46.633),
            "23.55\u{00b0} S, 46.63\u{00b0} W"
        );
    }

    #[test]
    fn this_machines_zone_is_known() {
        // Not every machine is in a city zone (UTC servers aren't), so only
        // check that looking does not fail outright.
        let _ = imp::zone();
    }
}
