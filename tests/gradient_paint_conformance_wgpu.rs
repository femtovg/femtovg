//! Headless GPU conformance tests for gradient *painting* — the coverage the
//! existing gradient tests miss: gradient on strokes and on text, multi-stop
//! LINEAR and RADIAL fills (distinct from the conic LUT path), and — the
//! important one — that gradient coordinates live in absolute user space, not
//! the painted shape's bounding box.
//!
//! Scenarios mirror WPT `html/canvas/element/fill-and-stroke-styles/2d.gradient.*`
//! and pin regressions shipped by other engines: bbox-remapping (WPT
//! `linear.transform.1` / `interpolate.outside`), radial edge clamping
//! (Mozilla bug 687188), gradient-on-text collapsing to a solid/per-glyph
//! (WebKit bug 24687, Mozilla 424586), and gradient-on-stroke parity with fill
//! (ThorVG #191/#501). Values cross-checked against Chrome. Each test skips when
//! no GPU adapter is available.
#![cfg(all(feature = "wgpu", feature = "textlayout"))]

use femtovg::{renderer::WGPURenderer, Align, Baseline, Canvas, Color, Paint, Path};

const W: u32 = 300;
const H: u32 = 120;
const FONT: &[u8] = include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf");

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("femtovg gradient paint test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grad paint out"),
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
    let mut pixels = vec![0u8; (unpadded * H) as usize];
    for row in 0..H as usize {
        let s = row * padded as usize;
        let d = row * unpadded as usize;
        pixels[d..d + unpadded as usize].copy_from_slice(&mapped[s..s + unpadded as usize]);
    }
    drop(mapped);
    readback.unmap();
    pixels
}

fn px(pixels: &[u8], x: usize, y: usize) -> [i32; 3] {
    let i = (y * W as usize + x) * 4;
    [pixels[i] as i32, pixels[i + 1] as i32, pixels[i + 2] as i32]
}
fn near(a: [i32; 3], b: [i32; 3], tol: i32) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
}

/// Gradient coordinates are in absolute user space, NOT the shape's bounding box
/// (WPT `2d.gradient.interpolate.outside` / `linear.transform.1`). A gradient
/// defined only over x in [100, 200] must clamp to its end colours everywhere
/// else, and two shapes at different positions must show different slices — a
/// bbox-remapping renderer would paint the full gradient in every shape.
#[test]
fn gradient_is_positioned_in_absolute_user_space() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let red = [230, 40, 40];
    let blue = [40, 60, 230];
    let pixels = render(&device, &queue, |c| {
        // Gradient lives on x in [100, 200]; fill the whole canvas.
        let g = Paint::linear_gradient(100.0, 0.0, 200.0, 0.0, Color::rgb(230, 40, 40), Color::rgb(40, 60, 230));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    let y = H as usize / 2;
    assert!(
        near(px(&pixels, 40, y), red, 4),
        "left of the gradient line must clamp to the start colour, got {:?}",
        px(&pixels, 40, y)
    );
    assert!(
        near(px(&pixels, 260, y), blue, 4),
        "right of the gradient line must clamp to the end colour, got {:?}",
        px(&pixels, 260, y)
    );
    // Middle of [100,200] is a blend, distinct from both ends.
    let mid = px(&pixels, 150, y);
    assert!(
        !near(mid, red, 30) && !near(mid, blue, 30),
        "middle of the gradient must be a blend, got {mid:?}"
    );
    // A bbox-remap bug would show the same full sweep in every column; instead
    // x=40 and x=260 are the two clamped ends (already asserted distinct).
    assert_ne!(px(&pixels, 40, y), px(&pixels, 260, y));
}

/// Multi-stop LINEAR fill (the `renderImageGradient` LUT path). Mirrors WPT
/// `2d.gradient.interpolate.multiple`: yellow -> cyan -> magenta at 0/0.5/1,
/// with the interpolated midpoints straight (non-premultiplied), matching Chrome.
#[test]
fn multi_stop_linear_interpolates_like_canvas() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let g = Paint::linear_gradient_stops(
            0.0,
            0.0,
            200.0,
            0.0,
            [
                (0.0, Color::rgb(255, 255, 0)),
                (0.5, Color::rgb(0, 255, 255)),
                (1.0, Color::rgb(255, 0, 255)),
            ],
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, 200.0, H as f32);
        c.fill_path(&p, &g);
    });
    let y = H as usize / 2;
    // WPT expected (+/- a few for the dither): 127,255,127 | 0,255,255 | 127,127,255.
    assert!(
        near(px(&pixels, 50, y), [127, 255, 127], 4),
        "yellow->cyan mid, got {:?}",
        px(&pixels, 50, y)
    );
    assert!(
        near(px(&pixels, 100, y), [0, 255, 255], 4),
        "cyan stop, got {:?}",
        px(&pixels, 100, y)
    );
    assert!(
        near(px(&pixels, 150, y), [127, 127, 255], 4),
        "cyan->magenta mid, got {:?}",
        px(&pixels, 150, y)
    );
}

/// Radial fill clamps to the true stop colours outside its ring (Mozilla bug
/// 687188): a shape entirely inside the inner radius is uniformly the first
/// stop; a point outside the outer radius is the last stop.
#[test]
fn radial_gradient_clamps_to_stop_colors() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
    let pixels = render(&device, &queue, |c| {
        // Green at r=40, red at r=50: a tight ring far from the centre and edges.
        let g = Paint::radial_gradient(cx, cy, 40.0, 50.0, Color::rgb(0, 200, 0), Color::rgb(220, 0, 0));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &g);
    });
    // Centre (r=0, inside r=40) clamps to the first stop (green).
    assert!(
        near(px(&pixels, cx as usize, cy as usize), [0, 200, 0], 6),
        "inside the inner radius must be the first stop (green)"
    );
    // A corner (far outside r=50) clamps to the last stop (red).
    assert!(
        near(px(&pixels, 5, 5), [220, 0, 0], 6),
        "outside the outer radius must be the last stop (red), got {:?}",
        px(&pixels, 5, 5)
    );
}

/// A gradient painted on a STROKE samples the same user-space coordinates as the
/// same gradient painted as a FILL — it is not re-fit to the thin stroke outline
/// (ThorVG #191/#501). A horizontal gradient stroked across the canvas is red on
/// the left, blue on the right, just like a fill would be.
#[test]
fn gradient_stroke_uses_user_space_like_fill() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let mut g = Paint::linear_gradient(
            0.0,
            0.0,
            W as f32,
            0.0,
            Color::rgb(230, 40, 40),
            Color::rgb(40, 60, 230),
        );
        g.set_line_width(24.0);
        // A thick horizontal line across the middle.
        let mut p = Path::new();
        p.move_to(0.0, H as f32 / 2.0);
        p.line_to(W as f32, H as f32 / 2.0);
        c.stroke_path(&p, &g);
    });
    let y = H as usize / 2;
    let left = px(&pixels, 20, y);
    let right = px(&pixels, W as usize - 20, y);
    println!("stroke left={left:?} right={right:?}");
    // Left end reddish, right end bluish — the gradient rides absolute x across
    // the stroke, not compressed to the ~24px stroke's bbox.
    assert!(left[0] > left[2] + 60, "stroke left must be reddish, got {left:?}");
    assert!(right[2] > right[0] + 60, "stroke right must be bluish, got {right:?}");
}

/// A gradient painted on TEXT spans the whole run continuously, not per-glyph and
/// not collapsed to a solid colour (WebKit bug 24687, Mozilla 424586). The ink
/// colour must move red -> blue monotonically across the word.
#[test]
fn gradient_text_is_continuous_across_the_run() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render(&device, &queue, |c| {
        let f = c.add_font_mem(FONT).unwrap();
        let paint = Paint::linear_gradient(
            20.0,
            0.0,
            W as f32 - 20.0,
            0.0,
            Color::rgb(230, 40, 40),
            Color::rgb(40, 60, 230),
        )
        .with_font(&[f])
        .with_font_size(70.0)
        .with_text_align(Align::Center)
        .with_text_baseline(Baseline::Middle);
        // Uniform glyphs so sampling is stable across the run.
        c.fill_text(W as f32 / 2.0, H as f32 / 2.0, "MMMM", &paint).unwrap();
    });
    // Find the inkiest pixel in a vertical band at several x positions.
    let cy = H as usize / 2;
    let ink_at = |x: usize| -> Option<[i32; 3]> {
        let mut best: Option<[i32; 3]> = None;
        for y in cy.saturating_sub(28)..(cy + 28).min(H as usize) {
            let c = px(&pixels, x, y);
            if c[0] + c[1] + c[2] < 600 && best.map_or(true, |b| c.iter().sum::<i32>() < b.iter().sum()) {
                best = Some(c);
            }
        }
        best
    };
    let samples: Vec<[i32; 3]> = [70usize, 120, 180, 230].iter().filter_map(|&x| ink_at(x)).collect();
    println!("text ink samples: {samples:?}");
    assert!(samples.len() >= 3, "expected ink at several positions across the word");
    // Red decreases and blue increases from left to right — continuous gradient.
    assert!(
        samples.first().unwrap()[0] > samples.last().unwrap()[0] + 40,
        "red must fall left->right: {samples:?}"
    );
    assert!(
        samples.last().unwrap()[2] > samples.first().unwrap()[2] + 40,
        "blue must rise left->right: {samples:?}"
    );
}
