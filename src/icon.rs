//! Draws the icons: the sun, through the day.
//!
//! | face      | glyph                        | when                        |
//! |-----------|------------------------------|-----------------------------|
//! | `Day`     | the sun and its rays         | daytime colour              |
//! | `Evening` | the sun going down           | after sunset                |
//! | `Bedtime` | a crescent moon              | Bedtime mode                |
//! | `Paused`  | the sun, struck through      | paused: the screen as it is |
//!
//! Drawn from signed distance functions with 4x4 supersampling rather than
//! shipped as bitmaps, like the other apps in the family: crisp at any edge,
//! no image decoder, and a new face is a few lines.
//!
//! On macOS the tray result is a template image (black + alpha), which AppKit
//! tints to suit the menu bar. On Windows the sun and moon are filled with
//! the colour the screen is turning, so the tray says it at a glance.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Day,
    Evening,
    Bedtime,
    Paused,
}

/// What the tray asks for. Windows downsamples to the notification-area size
/// itself, and starting from 32 keeps 150% and 200% DPI sharp. On macOS it is
/// the @2x of a 16pt menu bar glyph.
pub const EDGE: u32 = 32;

const SS: u32 = 4;

type Rgb = [f32; 3];

/// A premultiplied RGBA sample.
type Rgba = [f32; 4];

/// The tray glyph's design grid: 17.5 units across, centred on the origin, so
/// a unit is about 1.8 px. Luz sits beside Sua Vez, Cafe, Porta and Cota in the
/// menu bar and they share one keyline: centred, about 26 of 32 px, with a
/// 2.4 px stroke.
const TRAY_GRID: f32 = 17.5;
/// Half the stroke, in design units: 1.2 px.
const W: f32 = 0.66;

/// How the tray glyph is coloured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ink {
    /// macOS: black + alpha, tinted by AppKit.
    Template,
    /// Windows, against a dark taskbar.
    Dark,
    /// Windows, against a light taskbar.
    Light,
}

/// RGBA8, `edge` x `edge`, straight alpha.
pub fn render(face: Face, dark_taskbar: bool, edge: u32) -> Vec<u8> {
    let ink = if cfg!(target_os = "macos") {
        Ink::Template
    } else if dark_taskbar {
        Ink::Dark
    } else {
        Ink::Light
    };
    paint(face, ink, edge)
}

fn paint(face: Face, ink: Ink, edge: u32) -> Vec<u8> {
    let half = TRAY_GRID / 2.0;
    raster(edge, |x, y, px| {
        tray(face, ink, x * half, y * half, px * half)
    })
}

/// The fill for a face on Windows: the light the screen is giving off.
fn fill(face: Face, ink: Ink) -> Option<Rgb> {
    let dark = ink == Ink::Dark;
    match (face, ink) {
        (_, Ink::Template) | (Face::Paused, _) => None,
        (Face::Day, _) => Some(if dark { hex(0xffd75e) } else { hex(0xf5b400) }),
        (Face::Evening, _) => Some(if dark { hex(0xff9d4d) } else { hex(0xf07a1a) }),
        (Face::Bedtime, _) => Some(if dark { hex(0xffc58a) } else { hex(0xe08a3c) }),
    }
}

/// The sun: a disc of radius 3, and eight rays.
fn sun(x: f32, y: f32) -> (f32, f32) {
    let disc = len(x, y) - 3.0;
    let mut rays = f32::MAX;
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let (s, c) = a.sin_cos();
        rays = rays.min(capsule(x, y, (c * 5.0, s * 5.0), (c * 6.5, s * 6.5), W));
    }
    (disc, rays)
}

/// One sample of the tray glyph, in design units.
fn tray(face: Face, ink: Ink, x: f32, y: f32, px: f32) -> Rgba {
    let fg: Rgb = match ink {
        Ink::Template => [0.0; 3],
        Ink::Dark => [0.95, 0.95, 0.95],
        Ink::Light => [0.12, 0.12, 0.12],
    };
    let fill = fill(face, ink);
    let mut out = [0.0; 4];
    // A filled shape with an outline: the fill under the stroke's inner half,
    // so the two meet without a seam.
    let shape = |out: &mut Rgba, body: f32, extra: f32| {
        if let Some(f) = fill {
            over(out, f, cover(body, px));
        }
        over(out, fg, cover((body.abs() - W).min(extra), px));
    };
    match face {
        Face::Day => {
            let (disc, rays) = sun(x, y);
            shape(&mut out, disc, rays);
        }
        Face::Evening => {
            // Wide and short, so matched to its neighbours by width and
            // centred by height: the horizon sits a little below the middle.
            let (x, y) = (x * 1.03, y * 1.03 - 1.1);
            let horizon = 2.2;
            let disc = (len(x, y - horizon) - 3.6).max(y - horizon);
            let line = capsule(x, y, (-6.6, horizon), (6.6, horizon), W);
            let mut rays = f32::MAX;
            for deg in [-90.0f32, -50.0, -130.0, -15.0, -165.0] {
                let (s, c) = deg.to_radians().sin_cos();
                let at = |r: f32| (c * r, horizon + s * r);
                rays = rays.min(capsule(x, y, at(5.3), at(6.6), W));
            }
            if let Some(f) = fill {
                over(&mut out, f, cover(disc, px));
            }
            // Only the dome is outlined; the horizon closes it.
            let dome = (len(x, y - horizon) - 3.6).abs() - W;
            let dome = dome.max(y - horizon);
            over(&mut out, fg, cover(dome.min(line).min(rays), px));
        }
        Face::Bedtime => {
            // A circle with a bite out of its upper right.
            let x = x + 0.6;
            let moon = (len(x, y) - 6.2).max(-(len(x - 3.3, y + 2.6) - 5.0));
            shape(&mut out, moon, f32::MAX);
        }
        Face::Paused => {
            let (disc, rays) = sun(x, y);
            let glyph = (disc.abs() - W).min(rays);
            // A stroke corner to corner, with a gap cut either side of it.
            let slash = capsule(x, y, (-6.0, -6.0), (6.0, 6.0), W);
            let gap = slash - 1.1;
            over(&mut out, fg, cover(glyph.max(-gap).min(slash), px));
        }
    }
    out
}

/// How far in from the edge the app icon's tile sits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    /// Edge to edge, for the .ico: Windows draws no frame of its own and
    /// every pixel counts at 16.
    Full,
    /// Apple's icon grid: an 824/1024 tile with room for its own shadow, so it
    /// sits the same size as every other app in the Dock and Finder.
    Mac,
}

/// The application icon (installer, Start menu, Finder, Dock).
pub fn render_app(edge: u32, tile: Tile) -> Vec<u8> {
    raster(edge, |x, y, px| app(x, y, px, tile))
}

fn hex(v: u32) -> Rgb {
    [(v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff].map(|c| c as f32 / 255.0)
}

/// One sample of the app icon, in unit space: [-1, 1] across the canvas.
///
/// Dusk: the sun half gone below the horizon under a sky running from night
/// at the top to amber at the horizon, with its rays: the tray glyph's
/// evening face, in colour.
fn app(x: f32, y: f32, px: f32, tile: Tile) -> Rgba {
    let night = hex(0x1c1a4a);
    let dusk = hex(0x8c2f5c);
    let amber = hex(0xf4893a);
    let ground_top = hex(0x2a1f4f);
    let ground_bottom = hex(0x120f2c);
    let sun_top = hex(0xfff3c4);
    let sun_low = hex(0xffc061);
    let glow = hex(0xffb067);

    let half = match tile {
        Tile::Full => 0.96,
        Tile::Mac => 824.0 / 1024.0,
    };
    let mut out = [0.0; 4];

    // The tile: a superellipse, close to the continuous corners macOS uses.
    if tile == Tile::Mac {
        let shadow = soft(squircle(x, y - 0.025, half), 0.06) * 0.35;
        over(&mut out, [0.0; 3], shadow);
    }
    let inside = cover(squircle(x, y, half), px);

    // Everything on the tile keeps its share of it whatever the padding.
    let k = half / (824.0 / 1024.0);
    let (x, y, px) = (x / k, y / k, px / k);
    const HORIZON: f32 = 0.30;
    const R: f32 = 0.36;

    let t = ((y + 1.0) / (1.0 + HORIZON)).clamp(0.0, 1.0);
    let sky = if t < 0.55 {
        mix(night, dusk, t / 0.55)
    } else {
        mix(dusk, amber, (t - 0.55) / 0.45)
    };
    let below = mix(ground_top, ground_bottom, (y - HORIZON) / 0.5);
    let ground = cover(HORIZON - y, px);
    over(&mut out, mix(sky, below, ground), inside);

    // The sun's glow on the sky, then the sun itself, cut by the horizon.
    let to_sun = len(x, y - HORIZON);
    let halo = (1.0 - ((to_sun - R) / 0.45).clamp(0.0, 1.0)).powi(2) * 0.55;
    over(&mut out, glow, halo * (1.0 - ground) * inside);
    let disc = (to_sun - R).max(y - HORIZON);
    let shade = mix(sun_top, sun_low, (y - (HORIZON - R)) / R);
    over(&mut out, shade, cover(disc, px) * inside);

    // Rays, as in the tray glyph.
    let mut rays = f32::MAX;
    for deg in [-90.0f32, -50.0, -130.0, -15.0, -165.0] {
        let (s, c) = deg.to_radians().sin_cos();
        let at = |r: f32| (c * r, HORIZON + s * r);
        rays = rays.min(capsule(x, y, at(0.50), at(0.66), 0.045));
    }
    over(&mut out, sun_top, cover(rays, px) * inside);

    // A thin line of light where the sun meets the ground.
    let line = capsule(x, y, (-0.62, HORIZON + 0.01), (0.62, HORIZON + 0.01), 0.018);
    let fade = (1.0 - (x.abs() / 0.62).powi(2)).max(0.0);
    over(&mut out, sun_low, cover(line, px) * fade * inside);
    out
}

/// Supersample `f` over an `edge` x `edge` canvas. `f` gets unit-space
/// coordinates, [-1, 1] across with y down, and the size of one sample in
/// the same units, and returns premultiplied RGBA.
fn raster(edge: u32, f: impl Fn(f32, f32, f32) -> Rgba) -> Vec<u8> {
    let n = (edge * SS) as f32;
    let px = 2.0 / n;
    let mut out = Vec::with_capacity((edge * edge * 4) as usize);
    for py in 0..edge {
        for pxi in 0..edge {
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = ((pxi * SS + sx) as f32 + 0.5) * px - 1.0;
                    let y = ((py * SS + sy) as f32 + 0.5) * px - 1.0;
                    let c = f(x, y, px);
                    for i in 0..4 {
                        acc[i] += c[i];
                    }
                }
            }
            let a = acc[3];
            if a <= 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                out.extend_from_slice(&[
                    to8(acc[0] / a),
                    to8(acc[1] / a),
                    to8(acc[2] / a),
                    to8(a / (SS * SS) as f32),
                ]);
            }
        }
    }
    out
}

/// Paint `rgb` at coverage `a` over a premultiplied sample.
fn over(dst: &mut Rgba, rgb: Rgb, a: f32) {
    for i in 0..3 {
        dst[i] = rgb[i] * a + dst[i] * (1.0 - a);
    }
    dst[3] = a + dst[3] * (1.0 - a);
}

/// Coverage of a shape at signed distance `d`, antialiased over one sample.
fn cover(d: f32, px: f32) -> f32 {
    (0.5 - d / px).clamp(0.0, 1.0)
}

/// A soft falloff across `blur`, for shadows.
fn soft(d: f32, blur: f32) -> f32 {
    (0.5 - d / blur).clamp(0.0, 1.0).powi(2)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn len(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// Superellipse of half-size `h`. Not an exact distance, but close enough
/// near the edge, which is the only place it is used for antialiasing.
fn squircle(x: f32, y: f32, h: f32) -> f32 {
    const N: f32 = 5.0;
    ((x / h).abs().powf(N) + (y / h).abs().powf(N)).powf(1.0 / N) * h - h
}

fn capsule(x: f32, y: f32, a: (f32, f32), b: (f32, f32), r: f32) -> f32 {
    let (pax, pay) = (x - a.0, y - a.1);
    let (bax, bay) = (b.0 - a.0, b.1 - a.1);
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    len(pax - bax * h, pay - bay * h) - r
}

/// Sizes the app icon is written at: everything an .ico, an .icns iconset
/// and the docs want, each drawn rather than resampled.
pub const APP_SIZES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 128, 256, 1024];

const FACES: [(&str, Face); 4] = [
    ("day", Face::Day),
    ("evening", Face::Evening),
    ("bedtime", Face::Bedtime),
    ("paused", Face::Paused),
];

/// `luz --dump-icons <dir>`: every tray face and every app icon size as raw
/// RGBA, named `<name>-<edge>.rgba`. `assets/make_icons.py` turns them into
/// icon.ico and the PNGs; nothing in the shipped app needs a PNG encoder.
///
/// The `glyph-*` faces are for the landing page, which shows the Windows
/// dark-taskbar colours whatever OS the dump runs on.
pub fn dump(dir: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, face) in FACES {
        for (theme, dark) in [("dark", true), ("light", false)] {
            for edge in [EDGE, 128] {
                std::fs::write(
                    format!("{dir}/tray-{name}-{theme}-{edge}.rgba"),
                    render(face, dark, edge),
                )?;
            }
        }
        std::fs::write(
            format!("{dir}/glyph-{name}-128.rgba"),
            paint(face, Ink::Dark, 128),
        )?;
    }
    for edge in APP_SIZES {
        std::fs::write(
            format!("{dir}/app-{edge}.rgba"),
            render_app(edge, Tile::Full),
        )?;
    }
    std::fs::write(format!("{dir}/mac-1024.rgba"), render_app(1024, Tile::Mac))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(face: Face) -> Vec<u8> {
        render(face, true, EDGE)
    }

    #[test]
    fn faces_are_distinct_and_have_clear_corners() {
        let all: Vec<_> = FACES.iter().map(|(_, f)| ink(*f)).collect();
        for (i, a) in all.iter().enumerate() {
            assert!(a.chunks(4).any(|p| p[3] > 0), "{:?} is blank", FACES[i]);
            assert_eq!(a[3], 0);
            assert_eq!(a[a.len() - 1], 0);
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    /// The rows and columns with any ink, as (first, last).
    fn extent(px: &[u8], edge: u32) -> ((u32, u32), (u32, u32)) {
        let (mut x0, mut x1, mut y0, mut y1) = (edge, 0, edge, 0);
        for (i, p) in px.chunks(4).enumerate() {
            if p[3] > 64 {
                let (x, y) = (i as u32 % edge, i as u32 / edge);
                (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
            }
        }
        ((x0, x1), (y0, y1))
    }

    #[test]
    fn glyphs_sit_on_the_family_keyline() {
        for (name, face) in FACES {
            let ((x0, x1), (y0, y1)) = extent(&ink(face), EDGE);
            let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
            assert!(w.max(h) >= 24 && w.max(h) <= 28, "{name}: {w}x{h}");
            // Centred, give or take a pixel.
            assert!((x0 as i32 - (EDGE - 1 - x1) as i32).abs() <= 2, "{name}");
            assert!((y0 as i32 - (EDGE - 1 - y1) as i32).abs() <= 2, "{name}");
        }
    }

    #[test]
    fn app_icon_has_rounded_transparent_corners_and_opaque_centre() {
        for tile in [Tile::Full, Tile::Mac] {
            let edge = 64;
            let px = render_app(edge, tile);
            assert_eq!(px[3], 0, "{tile:?}");
            let centre = ((edge / 2 * edge + edge / 2) * 4 + 3) as usize;
            assert_eq!(px[centre], 255, "{tile:?}");
        }
    }
}
