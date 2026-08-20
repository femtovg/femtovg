//! Headless GPU tests for arbitrary-path clipping (`Canvas::clip_path`):
//! stencil-plane clips must match scissor results for rects, clip circle
//! corners (the #292 class), intersect when nested, replay on restore,
//! honor the even-odd clip-rule, compose with clipped concave fills and
//! stencil strokes, and clip image blits routed off the unclipped fast
//! path. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, ImageFlags, Paint, Path, PixelFormat, RenderTarget};

const W: u32 = 64;
const H: u32 = 64;

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
        label: Some("femtovg clip test device"),
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
        label: Some("clip test target"),
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

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

fn red() -> Paint {
    Paint::color(Color::rgb(255, 0, 0))
}

fn full_red_rect(canvas: &mut Canvas<WGPURenderer>) {
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&p, &red());
}

const WHITE: [u8; 3] = [255, 255, 255];
const RED: [u8; 3] = [255, 0, 0];

/// A rectangular path clip must clip exactly like the scissor does.
#[test]
fn rect_clip_matches_scissor() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let clipped = render(&device, &queue, |canvas| {
        let mut clip = Path::new();
        clip.rect(16.0, 16.0, 24.0, 24.0);
        canvas.clip_path(&clip, FillRule::NonZero);
        full_red_rect(canvas);
    });
    let scissored = render(&device, &queue, |canvas| {
        canvas.scissor(16.0, 16.0, 24.0, 24.0);
        full_red_rect(canvas);
    });
    for (x, y) in [(20, 20), (39, 39), (10, 10), (50, 50), (20, 50), (50, 20)] {
        assert_eq!(
            px(&clipped, x, y),
            px(&scissored, x, y),
            "clip and scissor disagree at ({x},{y})"
        );
    }
}

/// A circular clip cuts the corners a bounding rect would keep (the class
/// the rounded-scissor blit fix #292 guarded).
#[test]
fn circle_clip_cuts_corners() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let mut clip = Path::new();
        clip.circle(32.0, 32.0, 20.0);
        canvas.clip_path(&clip, FillRule::NonZero);
        full_red_rect(canvas);
    });
    assert_eq!(px(&out, 32, 32), RED, "center paints");
    // Inside the bounding rect of the circle but outside the circle.
    assert_eq!(px(&out, 16, 16), WHITE, "corner is clipped");
    assert_eq!(px(&out, 48, 48), WHITE, "corner is clipped");
    // On-axis extremes stay painted.
    assert_eq!(px(&out, 32, 14), RED, "top of the circle paints");
}

/// Nested clips intersect; restore() replays the survivors.
#[test]
fn nested_clips_intersect_and_restore_replays() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let mut a = Path::new();
        a.rect(8.0, 8.0, 32.0, 32.0); // 8..40
        canvas.clip_path(&a, FillRule::NonZero);

        canvas.save();
        let mut b = Path::new();
        b.rect(24.0, 24.0, 32.0, 32.0); // 24..56; intersection 24..40
        canvas.clip_path(&b, FillRule::NonZero);
        full_red_rect(canvas); // paints only 24..40
        canvas.restore();

        // Back to clip A alone: paint blue, lands in 8..40.
        let mut p = Path::new();
        p.rect(0.0, 0.0, 20.0, 20.0);
        canvas.fill_path(&p, &Paint::color(Color::rgb(0, 0, 255)));
    });
    assert_eq!(px(&out, 32, 32), RED, "intersection painted red");
    assert_eq!(
        px(&out, 12, 32),
        WHITE,
        "inside A, outside B stays white for the red fill"
    );
    assert_eq!(px(&out, 44, 44), WHITE, "inside B, outside A never paints");
    assert_eq!(
        px(&out, 12, 12),
        [0, 0, 255],
        "after restore, clip A alone gates the blue fill"
    );
    assert_eq!(
        px(&out, 4, 4),
        WHITE,
        "outside A the blue fill is clipped even after restore"
    );
}

/// The even-odd clip-rule punches the hole out of a donut clip.
#[test]
fn evenodd_clip_rule_respects_holes() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let mut donut = Path::new();
        donut.circle(32.0, 32.0, 24.0);
        donut.circle(32.0, 32.0, 10.0);
        canvas.clip_path(&donut, FillRule::EvenOdd);
        full_red_rect(canvas);
    });
    assert_eq!(px(&out, 32, 16), RED, "ring paints");
    assert_eq!(px(&out, 32, 32), WHITE, "donut hole is clipped out");
    assert_eq!(px(&out, 4, 4), WHITE, "outside the donut is clipped");
}

/// A self-intersecting concave fill inside a clip: the winding bits and the
/// clip bit share the stencil without corrupting each other.
#[test]
fn clipped_concave_fill_keeps_winding_and_clip_separate() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let mut clip = Path::new();
        clip.rect(0.0, 0.0, 32.0, 64.0); // left half only
        canvas.clip_path(&clip, FillRule::NonZero);

        // Self-intersecting bowtie spanning the whole canvas: concave path.
        let mut bowtie = Path::new();
        bowtie.move_to(4.0, 8.0);
        bowtie.line_to(60.0, 56.0);
        bowtie.line_to(60.0, 8.0);
        bowtie.line_to(4.0, 56.0);
        bowtie.close();
        canvas.fill_path(&bowtie, &red());

        // A second fill AFTER the concave one must still be clipped
        // correctly (the concave pass must not have destroyed the clip bit).
        let mut p = Path::new();
        p.rect(0.0, 58.0, 64.0, 6.0);
        canvas.fill_path(&p, &Paint::color(Color::rgb(0, 128, 0)));
    });
    assert_eq!(px(&out, 12, 30), RED, "bowtie interior paints inside the clip");
    assert_eq!(px(&out, 52, 30), WHITE, "bowtie clipped on the right half");
    assert_eq!(px(&out, 16, 60), [0, 128, 0], "later fill still clipped correctly");
    assert_eq!(px(&out, 48, 60), WHITE, "later fill clipped on the right");
}

/// Strokes (incl. the stencil-stroke path) respect the clip.
#[test]
fn strokes_respect_the_clip() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let mut clip = Path::new();
        clip.rect(0.0, 0.0, 64.0, 32.0); // top half
        canvas.clip_path(&clip, FillRule::NonZero);
        let mut line = Path::new();
        line.move_to(32.0, 0.0);
        line.line_to(32.0, 64.0);
        canvas.stroke_path(&line, &red().with_line_width(8.0));
    });
    assert_eq!(px(&out, 32, 16), RED, "stroke paints inside the clip");
    assert_eq!(px(&out, 32, 48), WHITE, "stroke clipped in the bottom half");
}

/// Image fills that would take the unclipped blit fast path are routed
/// through the masked path while a clip is active.
#[test]
fn image_blit_respects_the_clip() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let image = canvas
            .create_image_empty(
                64,
                64,
                PixelFormat::Rgba8,
                ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
            )
            .unwrap();
        canvas.save();
        canvas.set_render_target(RenderTarget::Image(image));
        canvas.clear_rect(0, 0, 64, 64, Color::rgb(255, 0, 0));
        canvas.set_render_target(RenderTarget::Screen);
        canvas.restore();

        let mut clip = Path::new();
        clip.circle(32.0, 32.0, 16.0);
        canvas.clip_path(&clip, FillRule::NonZero);

        let mut p = Path::new();
        p.rect(0.0, 0.0, 64.0, 64.0);
        let mut paint = Paint::image(image, 0.0, 0.0, 64.0, 64.0, 0.0, 1.0);
        paint.set_anti_alias(false);
        canvas.fill_path(&p, &paint);
    });
    assert_eq!(px(&out, 32, 32), RED, "blit paints inside the clip circle");
    assert_eq!(px(&out, 6, 6), WHITE, "blit clipped outside the circle");
}
