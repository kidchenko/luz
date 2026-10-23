//! Colour temperature, as the three gains a gamma ramp multiplies by.
//!
//! The colour of a blackbody at a temperature comes from Krystek's (1985)
//! fit to the Planckian locus, good from 1000 K to 15000 K, converted to
//! linear sRGB. It is taken relative to 6500 K, so daylight leaves the screen
//! exactly as it was, and the brightest channel is kept at full. The result is
//! gamma-encoded, because a ramp works on the values sent to the screen rather
//! than on light. The numbers land within a few percent of the usual tables
//! (Tanner Helland's fit to Mitchell Charity's blackbody colours).

/// What the menus and config accept. f.lux goes down to 1200 K, its "Ember".
pub const MIN_K: u32 = 1200;
pub const MAX_K: u32 = 6500;
/// Neutral: the screen as it is without Luz.
pub const NEUTRAL_K: u32 = 6500;

/// Red, green and blue gains, each 0 to 1.
pub type Gains = [f64; 3];

/// Linear-light sRGB of a blackbody at `kelvin`, at a luminance of 1.
fn blackbody(kelvin: f64) -> [f64; 3] {
    let t = kelvin.clamp(1000.0, 15000.0);
    let u = (0.860117757 + 1.54118254e-4 * t + 1.28641212e-7 * t * t)
        / (1.0 + 8.42420235e-4 * t + 7.08145163e-7 * t * t);
    let v = (0.317398726 + 4.22806245e-5 * t + 4.20481691e-8 * t * t)
        / (1.0 - 2.89741816e-5 * t + 1.61456053e-7 * t * t);
    let d = 2.0 * u - 8.0 * v + 4.0;
    let (x, y) = (3.0 * u / d, 2.0 * v / d);
    let (cx, cy, cz) = (x / y, 1.0, (1.0 - x - y) / y);
    [
        3.2406 * cx - 1.5372 * cy - 0.4986 * cz,
        -0.9689 * cx + 1.8758 * cy + 0.0415 * cz,
        0.0557 * cx - 0.2040 * cy + 1.0570 * cz,
    ]
}

/// The gains for `kelvin`: 1, 1, 1 at 6500 K, less blue and green below.
pub fn gains(kelvin: f64) -> Gains {
    let white = blackbody(NEUTRAL_K as f64);
    let c = blackbody(kelvin);
    let rel = [0, 1, 2].map(|i| (c[i] / white[i]).max(0.0));
    let top = rel.iter().cloned().fold(f64::MIN, f64::max).max(1e-9);
    rel.map(|v| (v / top).powf(1.0 / 2.2))
}

/// The temperature `t` of the way from `a` to `b`, in mireds (a million over
/// the kelvins). The eye sees equal steps in mireds as equal, so a fade done
/// this way does not seem to hurry at the warm end and dawdle at the cool one.
pub fn mix(a: f64, b: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let (ma, mb) = (1e6 / a, 1e6 / b);
    1e6 / (ma + (mb - ma) * t)
}

/// What f.lux calls each of its presets, for the menu.
pub fn name(kelvin: u32) -> Option<&'static str> {
    Some(match kelvin {
        6500 => "Daylight",
        5500 => "Sunlight",
        5000 => "Morning",
        4200 => "Fluorescent",
        3400 => "Halogen",
        2700 => "Incandescent",
        2300 => "Sunset",
        1900 => "Candle",
        1200 => "Ember",
        _ => return None,
    })
}

/// "3400 K", with a thin space as the SI style has it.
pub fn label(kelvin: u32) -> String {
    format!("{kelvin}\u{2009}K")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Gains, b: Gains) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.06)
    }

    #[test]
    fn daylight_is_neutral() {
        let g = gains(6500.0);
        assert!(g.iter().all(|v| (v - 1.0).abs() < 1e-9), "{g:?}");
    }

    #[test]
    fn close_to_the_usual_tables() {
        // Tanner Helland's fit, which most screen tinters use in some form.
        assert!(near(gains(3400.0), [1.0, 0.74, 0.53]));
        assert!(near(gains(2700.0), [1.0, 0.65, 0.34]));
        assert!(near(gains(1900.0), [1.0, 0.52, 0.0]));
        assert!(near(gains(1200.0), [1.0, 0.34, 0.0]));
    }

    #[test]
    fn warmer_means_less_blue_and_green() {
        let mut last = [0.0; 3];
        for k in (1200..=6500).step_by(100) {
            let g = gains(k as f64);
            assert!(g[1] >= last[1] && g[2] >= last[2], "{k}");
            assert_eq!(g[0], 1.0, "{k}");
            last = g;
        }
    }

    #[test]
    fn mixes_in_mireds() {
        assert_eq!(mix(6500.0, 3400.0, 0.0), 6500.0);
        assert!((mix(6500.0, 3400.0, 1.0) - 3400.0).abs() < 1e-9);
        // Halfway in mireds sits below halfway in kelvins.
        assert!(mix(6500.0, 3400.0, 0.5) < 4950.0);
    }
}
