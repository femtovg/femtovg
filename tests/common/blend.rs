//! The Compositing and Blending specification's blend and composite on the
//! CPU, for the tests of `ImageFilter::Blend` and of a layer's blend mode.
use femtovg::BlendMode;

pub fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

pub fn clip_color(c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut o = c;
    if n < 0.0 {
        o = o.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        o = o.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    o
}

pub fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

pub fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

pub fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let mn = c[0].min(c[1]).min(c[2]);
    let mx = c[0].max(c[1]).max(c[2]);
    if mx > mn {
        c.map(|v| (v - mn) * s / (mx - mn))
    } else {
        [0.0; 3]
    }
}

pub fn hard_light(cb: f64, cs: f64) -> f64 {
    if cs <= 0.5 {
        cb * 2.0 * cs
    } else {
        let s = 2.0 * cs - 1.0;
        cb + s - cb * s
    }
}

pub fn separable(mode: BlendMode, cb: f64, cs: f64) -> f64 {
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::ColorDodge => {
            if cb <= 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        _ => unreachable!("non-separable"),
    }
}

pub fn blend(mode: BlendMode, cb: [f64; 3], cs: [f64; 3]) -> [f64; 3] {
    match mode {
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        _ => [0, 1, 2].map(|i| separable(mode, cb[i], cs[i])),
    }
}

/// The specification's composite of premultiplied `s` over premultiplied
/// `b` under `mode`, in RGBA8.
pub fn expected(mode: BlendMode, s: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    let f = |v: u8| v as f64 / 255.0;
    let (sa, ba) = (f(s[3]), f(b[3]));
    let unpremul = |c: [u8; 4], a: f64| {
        if a > 0.0 {
            [f(c[0]) / a, f(c[1]) / a, f(c[2]) / a].map(|v| v.clamp(0.0, 1.0))
        } else {
            [0.0; 3]
        }
    };
    let (cs, cb) = (unpremul(s, sa), unpremul(b, ba));
    let bl = blend(mode, cb, cs).map(|v| v.clamp(0.0, 1.0));
    let ao = sa + ba - sa * ba;
    let co = [0, 1, 2].map(|i| f(s[i]) * (1.0 - ba) + f(b[i]) * (1.0 - sa) + sa * ba * bl[i]);
    [co[0], co[1], co[2], ao].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

pub const ALL_MODES: [BlendMode; 16] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

/// Premultiplied source pixel: color and alpha vary across the image so
/// every mode's branches get exercised.
pub fn source_pixel(x: u32, y: u32) -> [u8; 4] {
    let a = 0.35 + 0.65 * (x as f64 / 15.0);
    let (r, g, b) = (x as f64 / 15.0, y as f64 / 15.0, 0.6);
    [r, g, b]
        .map(|c| (c * a * 255.0).round() as u8)
        .into_iter()
        .chain([(a * 255.0).round() as u8])
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

pub fn backdrop_pixel(x: u32, y: u32) -> [u8; 4] {
    let a = 0.5 + 0.5 * (y as f64 / 15.0);
    let (r, g, b) = (0.8, 1.0 - y as f64 / 15.0, x as f64 / 15.0);
    [r, g, b]
        .map(|c| (c * a * 255.0).round() as u8)
        .into_iter()
        .chain([(a * 255.0).round() as u8])
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}
