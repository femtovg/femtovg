//! Headless GPU tests: gradient stops interpolate in straight (unpremultiplied)
//! space, the interpolation Canvas 2D and SVG gradients apply - Chromium 149
//! renders `createLinearGradient` / `<linearGradient>` from transparent red to
//! opaque blue with midpoint (192, 158, 193) over white, the straight-space
//! mix, and SVG 1.1/2 define stop interpolation per channel on the straight
//! color plus stop-opacity. The multi-stop LUT already bakes texels this way
//! (interpolate straight, premultiply the texel); these tests pin the two-stop
//! shader path to the same space, endpoint hue included.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 256;
const H: u32 = 64;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gradient interpolation out"),
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
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
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

    let mut out = vec![0u8; (W * H * 4) as usize];
    for y in 0..H as usize {
        let src = y * padded as usize;
        let dst = y * (W * 4) as usize;
        out[dst..dst + (W * 4) as usize].copy_from_slice(&mapped[src..src + (W * 4) as usize]);
    }
    out
}

fn px(buf: &[u8], x: u32, y: u32) -> [i32; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i] as i32, buf[i + 1] as i32, buf[i + 2] as i32]
}

fn close(a: [i32; 3], b: [i32; 3], tol: i32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

const TRANSPARENT_RED: (u8, u8, u8) = (220, 40, 40);
const BLUE: (u8, u8, u8) = (40, 80, 220);

fn transparent_red() -> Color {
    Color::rgba(TRANSPARENT_RED.0, TRANSPARENT_RED.1, TRANSPARENT_RED.2, 0)
}

fn blue() -> Color {
    Color::rgb(BLUE.0, BLUE.1, BLUE.2)
}

/// The straight-space expectation at gradient fraction `t`, composited over
/// white: mix the straight channels, then alpha-blend.
fn straight_over_white(t: f32) -> [i32; 3] {
    let a = t;
    let mix = |c0: u8, c1: u8| {
        let straight = f32::from(c0) * (1.0 - t) + f32::from(c1) * t;
        (straight * a + 255.0 * (1.0 - a)).round() as i32
    };
    [
        mix(TRANSPARENT_RED.0, BLUE.0),
        mix(TRANSPARENT_RED.1, BLUE.1),
        mix(TRANSPARENT_RED.2, BLUE.2),
    ]
}

/// A two-stop gradient from transparent red to opaque blue must keep the red
/// hue while fading - the straight-space interpolation browsers apply. A
/// premultiplied mix loses the transparent stop's hue entirely (its
/// premultiplied channels are zero) and lands ~45/255 away at the midpoint.
#[test]
fn two_stop_transparent_stop_keeps_hue() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let paint = Paint::linear_gradient(0.0, 0.0, W as f32, 0.0, transparent_red(), blue());
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &paint);
    });
    for t in [0.25f32, 0.5, 0.75] {
        let x = (t * W as f32) as u32;
        let got = px(&out, x, H / 2);
        let want = straight_over_white(t);
        assert!(
            close(got, want, 3),
            "at t={t}: got {got:?}, straight-space expects {want:?} (Chromium 149 matches this)"
        );
    }
}

/// The two-stop shader path and the multi-stop LUT must agree: both are
/// documented as the same gradient, differing only in how many stops forced
/// the LUT. The LUT quantizes to 256 texels, so allow its rounding.
#[test]
fn two_stop_matches_multi_stop_lut() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let two = render(&device, &queue, |canvas| {
        let paint = Paint::linear_gradient(0.0, 0.0, W as f32, 0.0, transparent_red(), blue());
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &paint);
    });
    let lut = render(&device, &queue, |canvas| {
        // The duplicate trailing stop forces the multi-stop LUT path.
        let stops = vec![(0.0, transparent_red()), (1.0, blue()), (1.0, blue())];
        let paint = Paint::linear_gradient_stops(0.0, 0.0, W as f32, 0.0, stops);
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &paint);
    });
    for x in (8..W - 8).step_by(16) {
        let a = px(&two, x, H / 2);
        let b = px(&lut, x, H / 2);
        assert!(
            close(a, b, 3),
            "two-stop and LUT paths diverge at x={x}: {a:?} vs {b:?}"
        );
    }
}

/// Conic gradients share the two-stop shader path; the fragment opposite the
/// start angle sits at fraction 0.5 and must show the straight-space mix.
#[test]
fn conic_two_stop_interpolates_straight() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let paint = Paint::conic_gradient(W as f32 / 2.0, H as f32 / 2.0, transparent_red(), blue());
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &paint);
    });
    // Left of centre = angle PI = fraction 0.5 along the ramp.
    let got = px(&out, W / 2 - 20, H / 2);
    let want = straight_over_white(0.5);
    assert!(
        close(got, want, 3),
        "conic midpoint: got {got:?}, straight-space expects {want:?}"
    );
}
