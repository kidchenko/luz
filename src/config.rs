//! Persisted settings, as JSON in `%APPDATA%\Luz\config.json` on Windows and
//! `~/Library/Application Support/Luz/config.json` on macOS.
//!
//! Edits on disk are picked up within a second, which is also how `luz
//! location` reaches a copy already in the tray.

use crate::color::{MAX_K, MIN_K};
use crate::log::lerror;
use crate::util::parse_clock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// f.lux's two speeds.
pub const SLOW_FADE: u32 = 60 * 60;
pub const FAST_FADE: u32 = 20;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// While the sun is up.
    pub day_kelvin: u32,
    /// From sunset on.
    pub sunset_kelvin: u32,
    /// From bedtime until you wake, with Bedtime mode on.
    pub bedtime_kelvin: u32,
    /// Bedtime mode: warmer still from `bedtime_at` to `wake_at`.
    pub bedtime: bool,
    /// "23:00", local time.
    pub bedtime_at: String,
    /// "07:00". With Bedtime mode on, mornings start at sunrise or at this,
    /// whichever is later, so a summer dawn at 05:00 doesn't light up the
    /// screen before you are up.
    pub wake_at: String,
    /// How long each change takes: an hour, or f.lux's fast 20 seconds.
    pub fade_seconds: u32,
    /// Where you are. Unset, Luz goes by the time zone's city.
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// f.lux's shortcuts: Alt+End, Alt+PgUp/PgDn, Alt+Shift+PgUp/PgDn.
    pub hotkeys: bool,
    /// Paused until this Unix time; 0 when not.
    pub paused_until: u64,
}

impl Default for Config {
    fn default() -> Self {
        // f.lux's recommended colours.
        Self {
            day_kelvin: 6500,
            sunset_kelvin: 3400,
            bedtime_kelvin: 1900,
            bedtime: true,
            bedtime_at: "23:00".into(),
            wake_at: "07:00".into(),
            fade_seconds: SLOW_FADE,
            latitude: None,
            longitude: None,
            hotkeys: true,
            paused_until: 0,
        }
    }
}

impl Config {
    #[cfg(windows)]
    pub fn dir() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Luz"))
    }

    #[cfg(target_os = "macos")]
    pub fn dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/Luz"))
    }

    pub fn path() -> Option<PathBuf> {
        Self::dir().map(|d| d.join("config.json"))
    }

    pub fn modified() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::path()?).ok()?.modified().ok()
    }

    /// `None` for missing or unparsable, so an editor caught mid-save changes
    /// nothing.
    pub fn reload() -> Option<Self> {
        Self::parse(&std::fs::read_to_string(Self::path()?).ok()?).ok()
    }

    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        // Notepad and PowerShell both like to write a BOM; serde_json does not.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        serde_json::from_str::<Config>(text).map(Config::sanitised)
    }

    /// Never fails: a missing or corrupt file yields defaults.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        Self::parse(&text).unwrap_or_else(|e| {
            lerror!(
                "config at {} is unreadable ({e}); using defaults",
                path.display()
            );
            Self::default()
        })
    }

    /// Temp file + rename, so an interrupted save never leaves half a file.
    pub fn save(&self) -> std::io::Result<()> {
        let (Some(dir), Some(path)) = (Self::dir(), Self::path()) else {
            return Err(std::io::Error::other("no config directory in environment"));
        };
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)
    }

    pub fn sanitised(mut self) -> Self {
        let d = Self::default();
        for k in [
            &mut self.day_kelvin,
            &mut self.sunset_kelvin,
            &mut self.bedtime_kelvin,
        ] {
            *k = (*k).clamp(MIN_K, MAX_K);
        }
        if parse_clock(&self.bedtime_at).is_none() {
            self.bedtime_at = d.bedtime_at;
        }
        if parse_clock(&self.wake_at).is_none() {
            self.wake_at = d.wake_at;
        }
        self.fade_seconds = self.fade_seconds.clamp(1, 3 * 60 * 60);
        // Half a location is none: both or neither.
        let lat_ok = self.latitude.is_some_and(|v| (-90.0..=90.0).contains(&v));
        let lon_ok = self
            .longitude
            .is_some_and(|v| (-180.0..=180.0).contains(&v));
        if !(lat_ok && lon_ok) {
            self.latitude = None;
            self.longitude = None;
        }
        self
    }

    /// Minutes after midnight.
    pub fn bedtime_minute(&self) -> u32 {
        parse_clock(&self.bedtime_at).unwrap_or(23 * 60)
    }

    pub fn wake_minute(&self) -> u32 {
        parse_clock(&self.wake_at).unwrap_or(7 * 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerates_bom_missing_fields_and_nonsense() {
        let c = Config::parse(
            "\u{feff}{\"sunsetKelvin\": 900, \"bedtimeAt\": \"25:00\", \"latitude\": 12.0}",
        )
        .unwrap();
        assert_eq!(c.sunset_kelvin, MIN_K);
        assert_eq!(c.bedtime_at, "23:00");
        assert_eq!(c.latitude, None, "no longitude to go with it");
        assert_eq!(Config::parse("{}").unwrap(), Config::default());
    }

    #[test]
    fn keeps_a_whole_location() {
        let c = Config::parse("{\"latitude\": -23.55, \"longitude\": -46.63}").unwrap();
        assert_eq!((c.latitude, c.longitude), (Some(-23.55), Some(-46.63)));
    }
}
