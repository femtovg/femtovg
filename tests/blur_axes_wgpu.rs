//! Headless GPU tests for the per-axis Gaussian blur (`GaussianBlur {
//! sigma_x, sigma_y }`): a blurred step edge must follow the discrete kernel
//! the shader runs on that axis - taps at whole pixels out to
//! ceil(3 sigma) - 1, Gaussian weights renormalised, composed over the
//! passes a split axis runs - and an edge across the other axis must stay a
//! step when that sigma is zero. The cases are the WPT canvas layer tests
//! `2d.layer.anisotropic-blur.{x-only,y-only,mostly-x}` (stdDeviation
//! [4, 0], [0, 4], [4, 1]) plus a split axis. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFilter, ImageFlags, LayerEffects, Paint, Path, PixelFormat};

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 128;
const H: u32 = 64;
/// The vertical step: columns below it black, from it on white.
const EDGE_X: u32 = 64;
/// The horizontal step: rows below it black, from it on white.
const EDGE_Y: u32 = 32;
/// The allowed deviation from the kernel model, in 8-bit counts: each pass
/// rounds its result to 8 bits.
const TOLERANCE: f64 = 2.5;

/// The shader's kernel for one pass of `sigma` on an axis: taps at whole
/// pixels up to ceil(3 sigma) - 1 either side, Gaussian weights normalised
/// over the taps; a degenerate sigma is the center tap alone.
fn kernel(sigma: f64) -> Vec<f64> {
    if sigma <= 0.0 {
        return vec![1.0];
    }
    let reach = (3.0 * sigma).ceil() as i64 - 1;
    let weights: Vec<f64> = (-reach..=reach)
        .map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f64 = weights.iter().sum();
    weights.into_iter().map(|w| w / sum).collect()
}

/// The kernels of `sigmas` composed, one pass each.
fn composed(sigmas: &[f64]) -> Vec<f64> {
    sigmas.iter().fold(vec![1.0], |acc, &sigma| {
        let k = kernel(sigma);
        let mut out = vec![0.0; acc.len() + k.len() - 1];
        for (i, a) in acc.iter().enumerate() {
            for (j, w) in k.iter().enumerate() {
                out[i + j] += a * w;
            }
        }
        out
    })
}

/// The white fraction the composed kernel leaves at the pixel `at` of a
/// step whose first white pixel is `edge`.
fn expected(kernel: &[f64], at: i64, edge: i64) -> f64 {
    let reach = (kernel.len() / 2) as i64;
    kernel
        .iter()
        .enumerate()
        .filter(|(i, _)| at + *i as i64 - reach >= edge)
        .map(|(_, w)| w)
        .sum()
}

fn red(buf: &[u8], x: u32, y: u32) -> f64 {
    buf[((y * W + x) * 4) as usize] as f64
}

/// The largest deviation of the middle row from the model of the x passes
/// `sigmas`, over the columns within three sigma 8 of the edge.
fn x_deviation(buf: &[u8], sigmas: &[f64]) -> f64 {
    let k = composed(sigmas);
    (16..=112)
        .map(|x| (red(buf, x, H / 2) - 255.0 * expected(&k, x as i64, EDGE_X as i64)).abs())
        .fold(0.0, f64::max)
}

/// The largest deviation of the middle column from the model of the y
/// passes `sigmas`, over the rows within three sigma 8 of the edge.
fn y_deviation(buf: &[u8], sigmas: &[f64]) -> f64 {
    let k = composed(sigmas);
    (8..=56)
        .map(|y| (red(buf, W / 2, y) - 255.0 * expected(&k, y as i64, EDGE_Y as i64)).abs())
        .fold(0.0, f64::max)
}

/// A step across `axis` (0: vertical edge at EDGE_X, 1: horizontal edge at
/// EDGE_Y), black before it, drawn without antialiasing over the whole
/// padded store a layer can hold.
fn step(canvas: &mut Canvas<WGPURenderer>, axis: usize) {
    let mut p = Path::new();
    if axis == 0 {
        p.rect(-400.0, -400.0, 400.0 + EDGE_X as f32, 800.0);
    } else {
        p.rect(-400.0, -400.0, 800.0, 400.0 + EDGE_Y as f32);
    }
    let mut paint = Paint::color(Color::black());
    paint.set_anti_alias(false);
    canvas.fill_path(&p, &paint);
}

/// Both steps under a layer blurred by `filter`: `(x deviation from the
/// model of the x passes, y deviation from the model of the y passes)`.
fn layer_deviations(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    filter: ImageFilter,
    passes: [&[f64]; 2],
) -> (f64, f64) {
    let scene = |axis: usize| {
        render_rgba(device, queue, W, H, Color::white(), |canvas| {
            assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[filter])));
            step(canvas, axis);
            canvas.end_layer();
        })
    };
    (x_deviation(&scene(0), passes[0]), y_deviation(&scene(1), passes[1]))
}

/// The WPT cases: x-only [4, 0] blurs the vertical edge by sigma 4 and
/// leaves the horizontal one a step, y-only [0, 4] the other way round, and
/// mostly-x [4, 1] gives each axis its own sigma.
#[test]
fn a_layer_blurs_each_axis_by_its_own_sigma() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for (name, sigma_x, sigma_y) in [("x-only", 4.0, 0.0), ("y-only", 0.0, 4.0), ("mostly-x", 4.0, 1.0)] {
        let filter = ImageFilter::GaussianBlur { sigma_x, sigma_y };
        let (dx, dy) = layer_deviations(&device, &queue, filter, [&[f64::from(sigma_x)], &[f64::from(sigma_y)]]);
        assert!(dx <= TOLERANCE, "{name}: {dx} counts off the x model");
        assert!(dy <= TOLERANCE, "{name}: {dy} counts off the y model");
        eprintln!("{name}: {dx:.2} / {dy:.2} counts off the x / y models");
    }
}

/// An axis above the per-pass bound splits on its own: sigma 16 along x is
/// four passes of 8 composed, while y, blurred by nothing, stays a step
/// through all four - in a layer, in a chain over an image, and in the
/// single pass, which clamps x to the bound and still copies y through.
#[test]
fn a_split_axis_composes_while_the_other_stays_sharp() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let filter = ImageFilter::GaussianBlur {
        sigma_x: 16.0,
        sigma_y: 0.0,
    };
    let four = [8.0; 4];
    let (dx, dy) = layer_deviations(&device, &queue, filter, [&four, &[0.0]]);
    assert!(dx <= TOLERANCE, "{dx} counts off four composed sigma-8 passes");
    assert!(dy <= TOLERANCE, "{dy} counts off the sharp y step");

    let chain = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        let (source, target) = images(canvas, 0, true);
        canvas.filter_image_chain(target, &[filter], source).expect("chain");
        blit(canvas, target);
    });
    let dx = x_deviation(&chain, &four);
    assert!(dx <= TOLERANCE, "chain: {dx} counts off four composed sigma-8 passes");
    // A lone blur pass keeps the upload's orientation, so its target samples
    // without FLIP_Y.
    let single = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        let (source, target) = images(canvas, 1, false);
        canvas.filter_image(target, filter, source);
        blit(canvas, target);
    });
    let dy = y_deviation(&single, &[0.0]);
    assert!(dy <= TOLERANCE, "single pass: {dy} counts off the sharp y step");
    let single = render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
        let (source, target) = images(canvas, 0, false);
        canvas.filter_image(target, filter, source);
        blit(canvas, target);
    });
    let dx = x_deviation(&single, &[8.0]);
    assert!(dx <= TOLERANCE, "single pass: {dx} counts off the clamped sigma-8 pass");
}

/// The step across `axis` as an uploaded image, and a filter target under
/// the chain's storage convention (`flip`) or the upload's.
fn images(canvas: &mut Canvas<WGPURenderer>, axis: usize, flip: bool) -> (femtovg::ImageId, femtovg::ImageId) {
    let pixels: Vec<femtovg::rgb::RGBA8> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let v = if (axis == 0 && x < EDGE_X) || (axis == 1 && y < EDGE_Y) {
                0
            } else {
                255
            };
            femtovg::rgb::RGBA8::new(v, v, v, 255)
        })
        .collect();
    let source = canvas
        .create_image(
            femtovg::imgref::Img::new(pixels.as_slice(), W as usize, H as usize),
            ImageFlags::empty(),
        )
        .expect("source image");
    let flags = if flip {
        ImageFlags::FLIP_Y | ImageFlags::PREMULTIPLIED
    } else {
        ImageFlags::PREMULTIPLIED
    };
    let target = canvas
        .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, flags)
        .expect("target image");
    (source, target)
}

fn blit(canvas: &mut Canvas<WGPURenderer>, image: femtovg::ImageId) {
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&p, &Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0));
}
