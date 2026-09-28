//! Headless GPU tests for blurs above the shader's per-pass bound (sigma 8:
//! the 24-tap loop GLES 2.0's constant loop bound allows). A chain, a layer
//! filter and a shadow split such a blur into passes that compose in
//! quadrature to the requested sigma, so a blurred step edge must follow the
//! analytic Gaussian profile of that sigma, 0.5 * (1 + erf(d / (sigma *
//! sqrt(2)))), where the single clamped pass renders a sigma-8 edge. Blurs
//! within the bound must stay the one pass they were. Skips without a GPU
//! adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    imgref::Img, renderer::WGPURenderer, rgb::RGBA8, Canvas, Color, ImageFilter, ImageFlags, ImageId, LayerEffects,
    Paint, Path, PixelFormat,
};

mod common;
use common::headless_device;

const W: u32 = 128;
const H: u32 = 64;
/// The step edge: columns below it are black, from it on white.
const EDGE: u32 = 64;
/// Columns checked against the profile, 3 sigma inside both image edges for
/// the sigmas below.
const CHECK: std::ops::RangeInclusive<u32> = 16..=112;
/// The allowed deviation from the analytic profile, in 8-bit counts.
const TOLERANCE: f64 = 3.0;

/// Renders `draw` on a white W x H canvas and reads back packed RGBA8 rows.
fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("blur test target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let renderer = WGPURenderer::new(device.clone(), queue.clone());
    let mut canvas = Canvas::new(renderer).expect("canvas");
    canvas.set_size(W, H, 1.0);
    canvas.clear_rect(0, 0, W, H, Color::white());

    draw(&mut canvas);

    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);

    let unpadded = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(enc.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = slice.get_mapped_range().expect("readback");
    let mut out = vec![0u8; (unpadded * H) as usize];
    for row in 0..H as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        out[dst..dst + unpadded as usize].copy_from_slice(&mapped[src..src + unpadded as usize]);
    }
    out
}

/// The step image the chain runners blur: every row is EDGE opaque black
/// pixels then opaque white ones.
fn step_image(canvas: &mut Canvas<WGPURenderer>) -> ImageId {
    let pixels: Vec<RGBA8> = (0..W * H)
        .map(|i| {
            let v = if i % W < EDGE { 0 } else { 255 };
            RGBA8::new(v, v, v, 255)
        })
        .collect();
    canvas
        .create_image(Img::new(pixels.as_slice(), W as usize, H as usize), ImageFlags::empty())
        .expect("source image")
}

/// A filter target under the chain's storage convention.
fn filter_target(canvas: &mut Canvas<WGPURenderer>) -> ImageId {
    canvas
        .create_image_empty(
            W as usize,
            H as usize,
            PixelFormat::Rgba8,
            ImageFlags::FLIP_Y | ImageFlags::PREMULTIPLIED,
        )
        .expect("target image")
}

/// Blits a filter target over the canvas.
fn blit(canvas: &mut Canvas<WGPURenderer>, image: ImageId) {
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&p, &Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0));
}

/// The step as a fill: black from far left of the canvas up to EDGE, over
/// every row a padded layer or shadow store can hold, drawn without
/// antialiasing so the edge is exactly at the pixel boundary.
fn step_fill(canvas: &mut Canvas<WGPURenderer>, y: f32, height: f32) {
    let mut p = Path::new();
    p.rect(-400.0, y, 400.0 + EDGE as f32, height);
    let mut paint = Paint::color(Color::black());
    paint.set_anti_alias(false);
    canvas.fill_path(&p, &paint);
}

/// erf by Abramowitz and Stegun 7.1.26: absolute error below 1.5e-7, three
/// orders under an 8-bit count.
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let y = 1.0 - poly * (-x * x).exp();
    if x < 0.0 {
        -y
    } else {
        y
    }
}

/// The white fraction a Gaussian of `sigma` leaves at column `x` of the step
/// edge, sampled at the pixel center.
fn profile(sigma: f64, x: u32) -> f64 {
    0.5 * (1.0 + erf((x as f64 + 0.5 - EDGE as f64) / (sigma * 2f64.sqrt())))
}

/// The largest deviation, in 8-bit counts, of the middle row's red channel
/// from the sigma profile over the checked columns.
fn max_deviation(buf: &[u8], sigma: f64) -> f64 {
    let row = H / 2;
    CHECK
        .map(|x| {
            let value = buf[((row * W + x) * 4) as usize] as f64;
            (value - 255.0 * profile(sigma, x)).abs()
        })
        .fold(0.0, f64::max)
}

/// `filter_image_chain` with sigma 16 renders the sigma-16 profile: four
/// passes of 8 compose in quadrature to one of 16. The single-pass
/// `filter_image`, which keeps the bound, renders the sigma-8 profile
/// instead and misses the sigma-16 one by tens of counts.
#[test]
fn a_split_blur_chain_matches_the_gaussian_profile() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let chain = render(&device, &queue, |canvas| {
        let source = step_image(canvas);
        let target = filter_target(canvas);
        canvas
            .filter_image_chain(target, &[ImageFilter::GaussianBlur { sigma: 16.0 }], source)
            .expect("chain");
        blit(canvas, target);
    });
    let deviation = max_deviation(&chain, 16.0);
    assert!(
        deviation <= TOLERANCE,
        "chain sigma 16 deviates from the sigma-16 profile by {deviation} counts"
    );

    let single = render(&device, &queue, |canvas| {
        let source = step_image(canvas);
        let target = filter_target(canvas);
        canvas.filter_image(target, ImageFilter::GaussianBlur { sigma: 16.0 }, source);
        blit(canvas, target);
    });
    let clamped = max_deviation(&single, 16.0);
    assert!(
        clamped > 30.0,
        "the clamped single pass should miss the sigma-16 profile by tens of counts, got {clamped}"
    );
    let at_bound = max_deviation(&single, 8.0);
    assert!(
        at_bound <= TOLERANCE,
        "the single pass renders sigma 8: deviates from its profile by {at_bound} counts"
    );
    eprintln!("chain sigma 16: {deviation:.2} counts off the sigma-16 profile; single pass: {clamped:.2} off it, {at_bound:.2} off sigma 8");
}

/// A layer filter with sigma 20 renders the sigma-20 profile: its store pads
/// by the true reach and its seven passes compose to the declared blur.
#[test]
fn a_split_blur_layer_matches_the_gaussian_profile() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let layered = render(&device, &queue, |canvas| {
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 20.0 }])));
        step_fill(canvas, -400.0, 800.0);
        canvas.end_layer();
    });
    let deviation = max_deviation(&layered, 20.0);
    assert!(
        deviation <= TOLERANCE,
        "layer sigma 20 deviates from the sigma-20 profile by {deviation} counts"
    );
    eprintln!("layer sigma 20: {deviation:.2} counts off the sigma-20 profile");
}

/// A shadow with `shadowBlur` 40 (sigma 20, beyond the 16 one pass covers)
/// renders the sigma-20 profile. The shape sits above the canvas and its
/// shadow is offset down onto it, so the middle row reads the shadow alone.
#[test]
fn a_large_shadow_blur_matches_the_gaussian_profile() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let shadowed = render(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::black());
        canvas.set_shadow_blur(40.0);
        canvas.set_shadow_offset(0.0, 300.0);
        step_fill(canvas, -400.0, 300.0);
    });
    let deviation = max_deviation(&shadowed, 20.0);
    assert!(
        deviation <= TOLERANCE,
        "shadow sigma 20 deviates from the sigma-20 profile by {deviation} counts"
    );
    eprintln!("shadow sigma 20: {deviation:.2} counts off the sigma-20 profile");
}

/// A blur within the bound is untouched by the split: the chain [blur
/// sigma] renders bit-identically to the single blur pass followed by the
/// parity identity it always was.
#[test]
fn a_blur_within_the_bound_is_bit_identical_to_its_single_pass() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for sigma in [3.0, 8.0] {
        let chain = render(&device, &queue, |canvas| {
            let source = step_image(canvas);
            let target = filter_target(canvas);
            canvas
                .filter_image_chain(target, &[ImageFilter::GaussianBlur { sigma }], source)
                .expect("chain");
            blit(canvas, target);
        });
        let single = render(&device, &queue, |canvas| {
            let source = step_image(canvas);
            let scratch = canvas
                .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED)
                .expect("scratch");
            let target = filter_target(canvas);
            canvas.filter_image(scratch, ImageFilter::GaussianBlur { sigma }, source);
            canvas.filter_image(target, ImageFilter::identity(), scratch);
            blit(canvas, target);
        });
        assert!(
            chain == single,
            "sigma {sigma}: the chain must be the single pass, bit for bit"
        );
        let deviation = max_deviation(&chain, f64::from(sigma));
        assert!(
            deviation <= TOLERANCE,
            "sigma {sigma}: {deviation} counts off its profile"
        );
    }
}
