//! Headless GPU tests for `ImageFilter::Turbulence` (SVG `feTurbulence`).
//!
//! Ground truth is a direct port of the reference implementation in the SVG
//! 1.1 specification (Filter Effects, section 15.19), which Chromium (Skia's
//! `SkPerlinNoiseShader`) and Firefox implement to the letter - including its
//! Park-Miller PRNG and its lattice construction - which is why two browsers
//! agree on every noise pixel. The GPU pass has to reproduce that same
//! lattice and arithmetic, so the tests compare it against the port, not
//! against a browser screenshot.
#![cfg(feature = "wgpu")]

mod common;
use common::headless_device;

// ---------------------------------------------------------------------------
// Reference implementation: SVG 1.1 section 15.19, transcribed. Kept in f64
// as the spec has it; the GPU runs f32 and the tests allow for that.
// ---------------------------------------------------------------------------

const RAND_M: i64 = 2147483647; // 2**31 - 1
const RAND_A: i64 = 16807; // 7**5; primitive root of m
const RAND_Q: i64 = 127773; // m / a
const RAND_R: i64 = 2836; // m % a
const B_SIZE: usize = 0x100;
const BM: i64 = 0xff;
const PERLIN_N: f64 = 0x1000 as f64;

fn setup_seed(mut seed: i64) -> i64 {
    if seed <= 0 {
        seed = -(seed % (RAND_M - 1)) + 1;
    }
    if seed > RAND_M - 1 {
        seed = RAND_M - 1;
    }
    seed
}

fn random(seed: i64) -> i64 {
    let mut result = RAND_A * (seed % RAND_Q) - RAND_R * (seed / RAND_Q);
    if result <= 0 {
        result += RAND_M;
    }
    result
}

struct Lattice {
    selector: [usize; B_SIZE + B_SIZE + 2],
    gradient: [[[f64; 2]; B_SIZE + B_SIZE + 2]; 4],
}

fn init(seed: i64) -> Lattice {
    let mut seed = setup_seed(seed);
    let mut lat = Lattice {
        selector: [0; B_SIZE + B_SIZE + 2],
        gradient: [[[0.0; 2]; B_SIZE + B_SIZE + 2]; 4],
    };
    for k in 0..4 {
        for i in 0..B_SIZE {
            lat.selector[i] = i;
            for j in 0..2 {
                seed = random(seed);
                lat.gradient[k][i][j] =
                    ((seed % (B_SIZE as i64 + B_SIZE as i64)) - B_SIZE as i64) as f64 / B_SIZE as f64;
            }
            let s =
                (lat.gradient[k][i][0] * lat.gradient[k][i][0] + lat.gradient[k][i][1] * lat.gradient[k][i][1]).sqrt();
            lat.gradient[k][i][0] /= s;
            lat.gradient[k][i][1] /= s;
        }
    }
    // `while (--i)` from BSize: shuffle indices BSize-1 down to 1.
    let mut i = B_SIZE;
    loop {
        i -= 1;
        if i == 0 {
            break;
        }
        let k = lat.selector[i];
        seed = random(seed);
        let j = (seed % B_SIZE as i64) as usize;
        lat.selector[i] = lat.selector[j];
        lat.selector[j] = k;
    }
    for i in 0..B_SIZE + 2 {
        lat.selector[B_SIZE + i] = lat.selector[i];
        for k in 0..4 {
            for j in 0..2 {
                lat.gradient[k][B_SIZE + i][j] = lat.gradient[k][i][j];
            }
        }
    }
    lat
}

fn s_curve(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[derive(Clone, Copy)]
struct Stitch {
    width: i64,
    height: i64,
    wrap_x: i64,
    wrap_y: i64,
}

fn noise2(lat: &Lattice, channel: usize, vec: [f64; 2], stitch: Option<Stitch>) -> f64 {
    let t = vec[0] + PERLIN_N;
    let mut bx0 = t as i64;
    let mut bx1 = bx0 + 1;
    let rx0 = t - (t as i64) as f64;
    let rx1 = rx0 - 1.0;
    let t = vec[1] + PERLIN_N;
    let mut by0 = t as i64;
    let mut by1 = by0 + 1;
    let ry0 = t - (t as i64) as f64;
    let ry1 = ry0 - 1.0;
    if let Some(s) = stitch {
        if bx0 >= s.wrap_x {
            bx0 -= s.width;
        }
        if bx1 >= s.wrap_x {
            bx1 -= s.width;
        }
        if by0 >= s.wrap_y {
            by0 -= s.height;
        }
        if by1 >= s.wrap_y {
            by1 -= s.height;
        }
    }
    let bx0 = (bx0 & BM) as usize;
    let bx1 = (bx1 & BM) as usize;
    let by0 = (by0 & BM) as usize;
    let by1 = (by1 & BM) as usize;
    let i = lat.selector[bx0];
    let j = lat.selector[bx1];
    let b00 = lat.selector[i + by0];
    let b10 = lat.selector[j + by0];
    let b01 = lat.selector[i + by1];
    let b11 = lat.selector[j + by1];
    let sx = s_curve(rx0);
    let sy = s_curve(ry0);
    let g = &lat.gradient[channel];
    let u = rx0 * g[b00][0] + ry0 * g[b00][1];
    let v = rx1 * g[b10][0] + ry0 * g[b10][1];
    let a = lerp(sx, u, v);
    let u = rx0 * g[b01][0] + ry1 * g[b01][1];
    let v = rx1 * g[b11][0] + ry1 * g[b11][1];
    let b = lerp(sx, u, v);
    lerp(sy, a, b)
}

#[allow(clippy::too_many_arguments)]
fn turbulence(
    lat: &Lattice,
    channel: usize,
    point: [f64; 2],
    mut base_freq: [f64; 2],
    num_octaves: u32,
    fractal_sum: bool,
    stitch_tile: Option<[f64; 4]>,
) -> f64 {
    let mut stitch = None;
    if let Some([tx, ty, tw, th]) = stitch_tile {
        // Adjust the base frequencies so the tile borders are continuous.
        for (axis, size) in [(0, tw), (1, th)] {
            let f = base_freq[axis];
            if f != 0.0 {
                let lo = (size * f).floor() / size;
                let hi = (size * f).ceil() / size;
                base_freq[axis] = if f / lo < hi / f { lo } else { hi };
            }
        }
        let width = (tw * base_freq[0] + 0.5) as i64;
        let height = (th * base_freq[1] + 0.5) as i64;
        stitch = Some(Stitch {
            width,
            wrap_x: (tx * base_freq[0] + PERLIN_N) as i64 + width,
            height,
            wrap_y: (ty * base_freq[1] + PERLIN_N) as i64 + height,
        });
    }
    let mut sum = 0.0;
    let mut vec = [point[0] * base_freq[0], point[1] * base_freq[1]];
    let mut ratio = 1.0;
    for _ in 0..num_octaves {
        let n = noise2(lat, channel, vec, stitch);
        sum += if fractal_sum { n } else { n.abs() } / ratio;
        vec[0] *= 2.0;
        vec[1] *= 2.0;
        ratio *= 2.0;
        if let Some(s) = stitch.as_mut() {
            s.width *= 2;
            s.wrap_x = 2 * s.wrap_x - PERLIN_N as i64;
            s.height *= 2;
            s.wrap_y = 2 * s.wrap_y - PERLIN_N as i64;
        }
    }
    sum
}

/// Unpremultiplied RGBA in [0, 1] for one pixel, per the spec's color mapping:
/// fractalNoise maps `(sum + 1) / 2`, turbulence uses `sum` directly.
fn reference_pixel(
    lat: &Lattice,
    point: [f64; 2],
    base_freq: [f64; 2],
    num_octaves: u32,
    fractal: bool,
    stitch_tile: Option<[f64; 4]>,
) -> [f64; 4] {
    let mut out = [0.0; 4];
    for (c, o) in out.iter_mut().enumerate() {
        let v = turbulence(lat, c, point, base_freq, num_octaves, fractal, stitch_tile);
        *o = (if fractal { (v + 1.0) / 2.0 } else { v }).clamp(0.0, 1.0);
    }
    out
}

// ---------------------------------------------------------------------------

/// The spec's own check value for its PRNG: from seed 1, the 10,000th number
/// is 1043618065. If this fails nothing downstream can match a browser.
#[test]
fn prng_matches_the_specifications_check_value() {
    let mut seed = setup_seed(1);
    for _ in 0..10_000 {
        seed = random(seed);
    }
    assert_eq!(seed, 1043618065);
}

/// The lattice is a deterministic function of the seed, gradients are unit
/// length, and the upper half mirrors the lower half as the spec lays it out.
#[test]
fn lattice_is_deterministic_and_well_formed() {
    let a = init(7);
    let b = init(7);
    assert_eq!(a.selector, b.selector);
    for k in 0..4 {
        for i in 0..B_SIZE + B_SIZE + 2 {
            assert_eq!(a.gradient[k][i], b.gradient[k][i]);
            let len = (a.gradient[k][i][0].powi(2) + a.gradient[k][i][1].powi(2)).sqrt();
            assert!((len - 1.0).abs() < 1e-12, "gradient {k}/{i} not unit: {len}");
        }
    }
    for i in 0..B_SIZE + 2 {
        assert_eq!(a.selector[B_SIZE + i], a.selector[i]);
    }
    // A permutation of 0..256 in the lower half.
    let mut seen = [false; B_SIZE];
    for &s in &a.selector[..B_SIZE] {
        assert!(!seen[s]);
        seen[s] = true;
    }
    // Different seeds give different lattices (seed 0 and 1 map to the same
    // start per setup_seed, so compare 7 against 11).
    let c = init(11);
    assert_ne!(a.selector, c.selector);
}

/// fractalNoise is centered on 0.5 and stays inside [0, 1]; turbulence is
/// non-negative. Sanity on the reference itself before it judges the GPU.
#[test]
fn reference_noise_has_the_expected_range() {
    let lat = init(1);
    let mut lo = 1.0f64;
    let mut hi = 0.0f64;
    let mut sum = 0.0;
    let n = 64 * 64;
    for y in 0..64 {
        for x in 0..64 {
            let p = reference_pixel(&lat, [x as f64, y as f64], [0.05, 0.05], 3, true, None);
            lo = lo.min(p[0]);
            hi = hi.max(p[0]);
            sum += p[0];
            let t = reference_pixel(&lat, [x as f64, y as f64], [0.05, 0.05], 3, false, None);
            assert!(t[0] >= 0.0);
        }
    }
    let mean = sum / n as f64;
    assert!(lo >= 0.0 && hi <= 1.0);
    assert!((mean - 0.5).abs() < 0.05, "fractalNoise mean {mean}, expected ~0.5");
    assert!(hi - lo > 0.2, "noise is flat: {lo}..{hi}");
}

// ---------------------------------------------------------------------------
// GPU pass against the reference.
// ---------------------------------------------------------------------------

use femtovg::{Color, ImageFilter, ImageFlags, Paint, Path, PixelFormat, Transform2D, TurbulenceKind};

fn noise_with(
    kind: TurbulenceKind,
    base_frequency: [f32; 2],
    num_octaves: u32,
    seed: i32,
    stitch_tiles: bool,
    transform: Transform2D,
) -> ImageFilter {
    ImageFilter::Turbulence {
        base_frequency,
        num_octaves,
        seed,
        stitch_tiles,
        kind,
        transform,
    }
}

fn noise(kind: TurbulenceKind, base_frequency: [f32; 2], num_octaves: u32, seed: i32) -> ImageFilter {
    noise_with(kind, base_frequency, num_octaves, seed, false, Transform2D::identity())
}

/// Runs `filters` as a chain over a `w` x `h` transparent source under the
/// documented target convention, draws the target 1:1 onto a transparent
/// output and returns its premultiplied RGBA8 pixels, rows top to bottom.
fn render_chain(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, filters: &[ImageFilter]) -> Vec<u8> {
    common::render_rgba(device, queue, w, h, Color::rgba(0, 0, 0, 0), |canvas| {
        let source = canvas
            .create_image_empty(w as usize, h as usize, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED)
            .expect("source image");
        let target = canvas
            .create_image_empty(
                w as usize,
                h as usize,
                PixelFormat::Rgba8,
                ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y | ImageFlags::NEAREST,
            )
            .expect("target image");
        canvas.filter_image_chain(target, filters, source);
        let mut p = Path::new();
        p.rect(0.0, 0.0, w as f32, h as f32);
        canvas.fill_path(&p, &Paint::image(target, 0.0, 0.0, w as f32, h as f32, 0.0, 1.0));
    })
}

fn at(px: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// The reference pixel as the premultiplied RGBA8 the GPU pass stores.
fn reference_rgba8(p: [f64; 4]) -> [f64; 4] {
    [
        p[0] * p[3] * 255.0,
        p[1] * p[3] * 255.0,
        p[2] * p[3] * 255.0,
        p[3] * 255.0,
    ]
}

/// Compares every pixel of a render against the reference and returns the
/// worst and mean absolute channel error in 8-bit steps.
fn compare(px: &[u8], w: u32, h: u32, reference: impl Fn(u32, u32) -> [f64; 4]) -> (f64, f64) {
    let mut worst = 0.0f64;
    let mut total = 0.0f64;
    for y in 0..h {
        for x in 0..w {
            let got = at(px, w, x, y);
            let want = reference_rgba8(reference(x, y));
            for c in 0..4 {
                let err = (got[c] as f64 - want[c]).abs();
                worst = worst.max(err);
                total += err;
            }
        }
    }
    (worst, total / (w * h * 4) as f64)
}

/// The lattice stores gradients at 8 bits (see src/turbulence.rs), which
/// bounds the noise sum's deviation below 2/255 before the output rounds;
/// premultiplying two such channels adds a little more. Anything beyond this
/// is a structural mistake (wrong corner, wrong orientation, wrong octave),
/// which shows up as errors in the tens.
const LATTICE_TOLERANCE: f64 = 4.0;

#[test]
fn fractal_noise_matches_the_reference() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (w, h) = (64, 48);
    let lat = init(1);
    let px = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::FractalNoise, [0.05, 0.08], 3, 1)],
    );
    let (worst, mean) = compare(&px, w, h, |x, y| {
        reference_pixel(&lat, [x as f64, y as f64], [0.05, 0.08], 3, true, None)
    });
    println!("fractalNoise vs reference: worst {worst:.1}/255, mean {mean:.3}/255");
    assert!(worst <= LATTICE_TOLERANCE, "worst channel error {worst}/255");
    assert!(mean <= 1.0, "mean channel error {mean}/255");
    // And it is noise, not a constant: the alpha channel varies across the image.
    let alphas: std::collections::BTreeSet<u8> = (0..w).map(|x| at(&px, w, x, h / 2)[3]).collect();
    assert!(alphas.len() > 8, "alpha barely varies: {alphas:?}");
}

#[test]
fn turbulence_matches_the_reference() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (w, h) = (64, 48);
    let lat = init(7);
    let px = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::Turbulence, [0.1, 0.1], 4, 7)],
    );
    let (worst, mean) = compare(&px, w, h, |x, y| {
        reference_pixel(&lat, [x as f64, y as f64], [0.1, 0.1], 4, false, None)
    });
    println!("turbulence vs reference: worst {worst:.1}/255, mean {mean:.3}/255");
    assert!(worst <= LATTICE_TOLERANCE, "worst channel error {worst}/255");
    assert!(mean <= 1.0, "mean channel error {mean}/255");
}

/// Two seeds give different noise; the same seed gives the same noise again
/// (the lattice cache hands back the same texture).
#[test]
fn seed_selects_the_lattice() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (w, h) = (32, 32);
    let a = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::FractalNoise, [0.1, 0.1], 2, 3)],
    );
    let b = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::FractalNoise, [0.1, 0.1], 2, 4)],
    );
    let a2 = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::FractalNoise, [0.1, 0.1], 2, 3)],
    );
    assert_eq!(a, a2);
    let differing = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    assert!(
        differing > a.len() / 2,
        "seeds 3 and 4 differ in only {differing} bytes"
    );
}

/// The transform maps noise space onto pixels: under a 2x scale, output pixel
/// (2x, 2y) samples the same noise point as (x, y) does at identity, and
/// under a translation the pattern moves with it.
#[test]
fn transform_scales_and_moves_the_noise() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (w, h) = (64, 64);
    let identity = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::FractalNoise, [0.07, 0.05], 3, 5)],
    );

    let scaled = noise_with(
        TurbulenceKind::FractalNoise,
        [0.07, 0.05],
        3,
        5,
        false,
        Transform2D::scaling(2.0, 2.0),
    );
    let scaled = render_chain(&device, &queue, w, h, &[scaled]);
    for y in 0..h / 2 {
        for x in 0..w / 2 {
            let (want, got) = (at(&identity, w, x, y), at(&scaled, w, 2 * x, 2 * y));
            for c in 0..4 {
                assert!(
                    (want[c] as i32 - got[c] as i32).abs() <= 1,
                    "scaled ({},{}) = {got:?}, identity ({x},{y}) = {want:?}",
                    2 * x,
                    2 * y
                );
            }
        }
    }

    let moved = noise_with(
        TurbulenceKind::FractalNoise,
        [0.07, 0.05],
        3,
        5,
        false,
        Transform2D::translation(10.0, 6.0),
    );
    let moved = render_chain(&device, &queue, w, h, &[moved]);
    for y in 6..h {
        for x in 10..w {
            let (want, got) = (at(&identity, w, x - 10, y - 6), at(&moved, w, x, y));
            for c in 0..4 {
                assert!(
                    (want[c] as i32 - got[c] as i32).abs() <= 1,
                    "moved ({x},{y}) = {got:?}, identity ({},{}) = {want:?}",
                    x - 10,
                    y - 6
                );
            }
        }
    }
}

/// stitchTiles: the reference is periodic across the tile once the frequency
/// is snapped, and the GPU pass reproduces the stitched reference, so the
/// noise wraps seamlessly at the image edge.
#[test]
fn stitching_matches_the_stitched_reference_and_is_periodic() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (w, h) = (64, 64);
    let lat = init(2);
    let tile = Some([0.0, 0.0, w as f64, h as f64]);

    // The oracle's own seam: with the frequency snapped to whole periods and
    // the lattice wrapped at the tile edge, the noise one tile across (or
    // down) is the noise at the tile's start, so the edges meet seamlessly.
    // Unstitched, the same positions are unrelated.
    for y in [0.0, 5.0, 17.0, 63.0] {
        let edge = reference_pixel(&lat, [w as f64, y], [0.05, 0.05], 3, true, tile);
        let start = reference_pixel(&lat, [0.0, y], [0.05, 0.05], 3, true, tile);
        let unstitched_edge = reference_pixel(&lat, [w as f64, y], [0.05, 0.05], 3, true, None);
        let unstitched_start = reference_pixel(&lat, [0.0, y], [0.05, 0.05], 3, true, None);
        for ch in 0..4 {
            assert!((edge[ch] - start[ch]).abs() < 1e-9, "seam open at y={y}");
        }
        assert!(
            (0..4).any(|ch| (unstitched_edge[ch] - unstitched_start[ch]).abs() > 1e-3),
            "unstitched noise happens to match at y={y}"
        );
    }
    for x in [0.0, 5.0, 17.0, 63.0] {
        let edge = reference_pixel(&lat, [x, h as f64], [0.05, 0.05], 3, true, tile);
        let start = reference_pixel(&lat, [x, 0.0], [0.05, 0.05], 3, true, tile);
        for ch in 0..4 {
            assert!((edge[ch] - start[ch]).abs() < 1e-9, "seam open at x={x}");
        }
    }

    let stitched = noise_with(
        TurbulenceKind::FractalNoise,
        [0.05, 0.05],
        3,
        2,
        true,
        Transform2D::identity(),
    );
    let px = render_chain(&device, &queue, w, h, &[stitched]);
    let (worst, mean) = compare(&px, w, h, |x, y| {
        reference_pixel(&lat, [x as f64, y as f64], [0.05, 0.05], 3, true, tile)
    });
    println!("stitched fractalNoise vs reference: worst {worst:.1}/255, mean {mean:.3}/255");
    assert!(worst <= LATTICE_TOLERANCE, "worst channel error {worst}/255");

    // Stitched and unstitched renders differ (the frequency snapped from 0.05
    // to 3/64 and the lattice wraps), so the flag reached the shader.
    let plain = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::FractalNoise, [0.05, 0.05], 3, 2)],
    );
    assert_ne!(px, plain);
}

/// Zero octaves sum nothing: fractalNoise is a flat 50% gray at 50% alpha,
/// turbulence is transparent black.
#[test]
fn zero_octaves_is_the_constant_the_spec_gives() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (w, h) = (8, 8);
    let fractal = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::FractalNoise, [0.1, 0.1], 0, 1)],
    );
    let turbulence = render_chain(
        &device,
        &queue,
        w,
        h,
        &[noise(TurbulenceKind::Turbulence, [0.1, 0.1], 0, 1)],
    );
    for y in 0..h {
        for x in 0..w {
            let f = at(&fractal, w, x, y);
            assert!(
                (f[3] as i32 - 128).abs() <= 1 && (f[0] as i32 - 64).abs() <= 1,
                "fractal {f:?}"
            );
            assert_eq!(at(&turbulence, w, x, y), [0, 0, 0, 0]);
        }
    }
}

/// The corpus shape: noise, a color matrix that turns one channel into an
/// opacity under a constant tint, and the linearRGB-to-sRGB transfer that a
/// default SVG filter ends with. Checked end to end against the reference
/// pushed through the same arithmetic.
#[test]
fn chain_with_matrix_and_transfer_matches_the_reference() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (w, h) = (48, 48);
    let lat = init(7);
    // claude-opus-5's grain matrix: constant warm tint, alpha = 0.9 * red noise.
    #[rustfmt::skip]
    let matrix = [
        0.0, 0.0, 0.0, 0.0, 0.55,
        0.0, 0.0, 0.0, 0.0, 0.36,
        0.0, 0.0, 0.0, 0.0, 0.24,
        0.9, 0.0, 0.0, 0.0, 0.0,
    ];
    let px = render_chain(
        &device,
        &queue,
        w,
        h,
        &[
            noise(TurbulenceKind::FractalNoise, [0.85, 0.85], 4, 7),
            ImageFilter::ColorMatrix { matrix },
            ImageFilter::LinearRgbToSrgb,
        ],
    );
    let to_srgb = |v: f64| {
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    let (worst, mean) = compare(&px, w, h, |x, y| {
        let n = reference_pixel(&lat, [x as f64, y as f64], [0.85, 0.85], 4, true, None);
        // The matrix pass quantizes its output to 8 bits before the transfer.
        let q = |v: f64| (v * 255.0).round() / 255.0;
        let a = q((0.9 * n[0]).clamp(0.0, 1.0));
        [to_srgb(0.55), to_srgb(0.36), to_srgb(0.24), a]
    });
    println!("grain chain vs reference: worst {worst:.1}/255, mean {mean:.3}/255");
    // Three quantizing passes stack, so allow a little more than one.
    assert!(worst <= LATTICE_TOLERANCE + 2.0, "worst channel error {worst}/255");
    assert!(mean <= 1.0, "mean channel error {mean}/255");
}
