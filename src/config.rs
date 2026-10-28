//! Persisted settings, as JSON in `%APPDATA%\Luz\config.json` on Windows,
//! `~/Library/Application Support/Luz/config.json` on macOS and
//! `~/.config/luz/config.json` on Linux. traykit owns the file: where it
//! lives, the BOM Notepad adds, and saving it safely.
//!
//! Edits on disk are picked up within a second, which is also how `luz
//! location` reaches a copy already in the tray.

use crate::color::{MAX_K, MIN_K};
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
    pub fn path() -> Option<PathBuf> {
        traykit::config::path()
    }

    pub fn modified() -> Option<std::time::SystemTime> {
        traykit::config::modified()
    }

    /// `None` for missing or unparsable, so an editor caught mid-save changes
    /// nothing.
    pub fn reload() -> Option<Self> {
        traykit::config::reload::<Self>().map(Self::sanitised)
    }

    #[cfg(test)]
    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        traykit::config::parse::<Self>(text).map(Self::sanitised)
    }

    /// Never fails: a missing or corrupt file yields defaults.
    pub fn load() -> Self {
        traykit::config::load::<Self>().sanitised()
    }

    /// Temp file + rename, so an interrupted save never leaves half a file.
    pub fn save(&self) -> std::io::Result<()> {
        traykit::config::save(self)
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
