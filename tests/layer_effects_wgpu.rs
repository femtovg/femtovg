//! Headless GPU tests for layer capture and effects (`Canvas::begin_layer` /
//! `end_layer`): group opacity must fade the layer as one image (no
//! double-blending of overlapping children), filtered layers must not mirror
//! (the FLIP_Y storage-parity bookkeeping), declared blurs must actually
//! spread, nesting must multiply opacities, and the composite must honor the
//! outer scissor. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFilter, LayerEffects, Paint, Path};

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
        label: Some("femtovg layer test device"),
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
        label: Some("layer test target"),
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

fn close(a: u8, b: i32) -> bool {
    (a as i32 - b).abs() <= 6
}

fn red_rect(canvas: &mut Canvas<WGPURenderer>, x: f32, y: f32, w: f32, h: f32) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    canvas.fill_path(&p, &Paint::color(Color::rgb(255, 0, 0)));
}

/// Group opacity fades the layer as ONE image: where two opaque children
/// overlap, the composite shows the same 50% red as where only one child
/// painted - not the doubled coverage per-draw alpha produces.
#[test]
fn group_opacity_does_not_double_blend() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let layered = render(&device, &queue, |canvas| {
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.5));
        red_rect(canvas, 8.0, 8.0, 32.0, 32.0);
        red_rect(canvas, 24.0, 24.0, 32.0, 32.0); // overlaps the first
        canvas.end_layer();
    });
    let single = px(&layered, 12, 12); // covered by one child
    let overlap = px(&layered, 32, 32); // covered by both children
    assert!(
        close(single[0], 255) && close(single[1], 127),
        "single coverage should be 50% red over white, got {single:?}"
    );
    assert_eq!(
        overlap, single,
        "overlap must not double-blend: layer opacity fades the group as one image"
    );

    // Control: per-draw alpha DOES double-blend, proving the layer differs.
    let per_draw = render(&device, &queue, |canvas| {
        canvas.set_global_alpha(0.5);
        red_rect(canvas, 8.0, 8.0, 32.0, 32.0);
        red_rect(canvas, 24.0, 24.0, 32.0, 32.0);
    });
    let overlap_pd = px(&per_draw, 32, 32);
    assert!(
        overlap_pd[1] < 96,
        "per-draw alpha overlap should be darker than 50%, got {overlap_pd:?}"
    );
}

/// A filtered layer must come out upright: the capture holds flipped storage
/// and the chain flips parity once, so the composite samples the filtered
/// result without FLIP_Y. Red-on-top must stay on top.
#[test]
fn filtered_layer_is_not_mirrored() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        canvas.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::brightness(1.0)]));
        red_rect(canvas, 0.0, 0.0, 64.0, 24.0);
        let mut p = Path::new();
        p.rect(0.0, 40.0, 64.0, 24.0);
        canvas.fill_path(&p, &Paint::color(Color::rgb(0, 0, 255)));
        canvas.end_layer();
    });
    let top = px(&out, 32, 8);
    let bottom = px(&out, 32, 56);
    assert!(
        close(top[0], 255) && close(top[2], 0),
        "top should stay red, got {top:?}"
    );
    assert!(
        close(bottom[2], 255) && close(bottom[0], 0),
        "bottom should stay blue, got {bottom:?} - a swap means the filtered layer mirrored"
    );
}

/// A blur declared at begin_layer actually spreads: a hard edge inside the
/// layer softens, and content near the scissor edge keeps its blur reach
/// thanks to the declared-filter padding.
#[test]
fn declared_blur_applies_and_pads() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let blurred = render(&device, &queue, |canvas| {
        canvas.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 3.0 }]));
        red_rect(canvas, 16.0, 16.0, 32.0, 32.0);
        canvas.end_layer();
    });
    // Just outside the rect edge: a hard edge leaves it white, a blur tints it.
    let outside = px(&blurred, 52, 32);
    assert!(
        outside[1] < 250,
        "blur should reach past the rect edge, got {outside:?}"
    );
    // Center stays red.
    let center = px(&blurred, 32, 32);
    assert!(close(center[0], 255), "center should stay red-ish, got {center:?}");
}

/// Nested layers multiply their opacities; the composite of the inner layer
/// happens inside the outer capture.
#[test]
fn nested_layers_multiply_opacity() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.5));
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.5));
        red_rect(canvas, 8.0, 8.0, 48.0, 48.0);
        canvas.end_layer();
        canvas.end_layer();
    });
    let center = px(&out, 32, 32);
    // 25% red over white: r=255, g=b=191.
    assert!(
        close(center[0], 255) && close(center[1], 191),
        "nested 0.5 x 0.5 should show 25% red, got {center:?}"
    );
}

/// The composite honors the scissor in effect at begin_layer: layer content
/// cannot escape it, even though the layer itself resets the scissor inside.
#[test]
fn layer_composite_honors_outer_scissor() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        canvas.save();
        canvas.scissor(16.0, 16.0, 24.0, 24.0);
        canvas.begin_layer(&LayerEffects::new());
        red_rect(canvas, 0.0, 0.0, 64.0, 64.0); // fills well past the scissor
        canvas.end_layer();
        canvas.restore();
    });
    assert!(
        close(px(&out, 20, 20)[0], 255) && close(px(&out, 20, 20)[1], 0),
        "inside the scissor should be red"
    );
    assert_eq!(px(&out, 50, 50), [255, 255, 255], "outside the scissor must stay white");
}

fn circle_mask_image(canvas: &mut Canvas<WGPURenderer>) -> femtovg::ImageId {
    // White circle on transparent: full luminance coverage inside, none outside.
    let mask = canvas
        .create_image_empty(
            48,
            48,
            femtovg::PixelFormat::Rgba8,
            femtovg::ImageFlags::PREMULTIPLIED | femtovg::ImageFlags::FLIP_Y,
        )
        .unwrap();
    canvas.save();
    canvas.set_render_target(femtovg::RenderTarget::Image(mask));
    canvas.clear_rect(0, 0, 48, 48, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
    canvas.reset_transform();
    let mut p = Path::new();
    p.circle(24.0, 24.0, 20.0);
    canvas.fill_path(&p, &Paint::color(Color::white()));
    canvas.set_render_target(femtovg::RenderTarget::Screen);
    canvas.restore();
    mask
}

/// A luminance mask shows the layer inside its white region and hides it
/// outside (uncovered pixels mask out fully) - SVG mask semantics.
#[test]
fn luminance_mask_gates_the_layer() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        let mask = circle_mask_image(canvas);
        canvas.begin_layer(&LayerEffects::new().with_mask(mask, femtovg::MaskKind::Luminance, 8.0, 8.0, 48.0, 48.0));
        red_rect(canvas, 0.0, 0.0, 64.0, 64.0);
        canvas.end_layer();
    });
    let inside = px(&out, 32, 32); // circle centre (mask at 8..56)
    let outside_circle = px(&out, 12, 12); // inside mask rect, outside circle
    let outside_rect = px(&out, 60, 60); // outside the mask rect entirely
    assert!(
        close(inside[0], 255) && close(inside[1], 0),
        "inside the mask circle the layer shows, got {inside:?}"
    );
    assert_eq!(
        outside_circle,
        [255, 255, 255],
        "outside the circle the layer is masked out"
    );
    assert_eq!(
        outside_rect,
        [255, 255, 255],
        "beyond the mask rect the layer is masked out"
    );
}

/// A black region of a luminance mask hides content even though its alpha is
/// opaque - proving coverage is luminance, not alpha.
#[test]
fn luminance_mask_uses_luminance_not_alpha() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        // Opaque half-white / half-black mask.
        let mask = canvas
            .create_image_empty(
                64,
                64,
                femtovg::PixelFormat::Rgba8,
                femtovg::ImageFlags::PREMULTIPLIED | femtovg::ImageFlags::FLIP_Y,
            )
            .unwrap();
        canvas.save();
        canvas.set_render_target(femtovg::RenderTarget::Image(mask));
        canvas.clear_rect(0, 0, 64, 64, Color::black());
        canvas.reset_transform();
        let mut p = Path::new();
        p.rect(0.0, 0.0, 32.0, 64.0);
        canvas.fill_path(&p, &Paint::color(Color::white()));
        canvas.set_render_target(femtovg::RenderTarget::Screen);
        canvas.restore();

        canvas.begin_layer(&LayerEffects::new().with_mask(mask, femtovg::MaskKind::Luminance, 0.0, 0.0, 64.0, 64.0));
        red_rect(canvas, 0.0, 0.0, 64.0, 64.0);
        canvas.end_layer();
    });
    assert!(
        close(px(&out, 16, 32)[0], 255) && close(px(&out, 16, 32)[1], 0),
        "white mask half shows the layer"
    );
    assert_eq!(
        px(&out, 48, 32),
        [255, 255, 255],
        "black (but opaque) mask half hides the layer - luminance, not alpha"
    );
}

/// Masking a FILTERED layer keeps both the mask and the content upright -
/// the storage-parity flag selection for the filtered case.
#[test]
fn masked_filtered_layer_keeps_orientation() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        // Mask: white on the TOP half only.
        let mask = canvas
            .create_image_empty(
                64,
                64,
                femtovg::PixelFormat::Rgba8,
                femtovg::ImageFlags::PREMULTIPLIED | femtovg::ImageFlags::FLIP_Y,
            )
            .unwrap();
        canvas.save();
        canvas.set_render_target(femtovg::RenderTarget::Image(mask));
        canvas.clear_rect(0, 0, 64, 64, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
        canvas.reset_transform();
        let mut p = Path::new();
        p.rect(0.0, 0.0, 64.0, 32.0);
        canvas.fill_path(&p, &Paint::color(Color::white()));
        canvas.set_render_target(femtovg::RenderTarget::Screen);
        canvas.restore();

        canvas.begin_layer(
            &LayerEffects::new()
                .with_filters(&[ImageFilter::brightness(1.0)])
                .with_mask(mask, femtovg::MaskKind::Luminance, 0.0, 0.0, 64.0, 64.0),
        );
        // Red on top, blue on bottom.
        red_rect(canvas, 0.0, 0.0, 64.0, 32.0);
        let mut p = Path::new();
        p.rect(0.0, 32.0, 64.0, 32.0);
        canvas.fill_path(&p, &Paint::color(Color::rgb(0, 0, 255)));
        canvas.end_layer();
    });
    let top = px(&out, 32, 12);
    let bottom = px(&out, 32, 52);
    assert!(
        close(top[0], 255) && close(top[2], 0),
        "top-half mask over a filtered layer must show the RED top, got {top:?}"
    );
    assert_eq!(
        bottom,
        [255, 255, 255],
        "bottom must be masked out, got {bottom:?} - blue here means the mask or content mirrored"
    );
}

/// A luminance mask's coverage is luminance x alpha (SVG mask semantics):
/// white fading to transparent must fade the layer out, not hold it at
/// full coverage the way a straight luminanceToAlpha conversion would.
/// Regression for background-noodles-left-dark.svg, whose white->transparent
/// gradient mask was ignored entirely.
#[test]
fn luminance_mask_multiplies_alpha() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        canvas.clear_rect(0, 0, W, H, Color::white());

        // Canvas-sized white->transparent vertical fade, captured mid-frame
        // the way SVG integrations rasterize <mask> content.
        let mask = canvas
            .create_image_empty(
                W as usize,
                H as usize,
                femtovg::PixelFormat::Rgba8,
                femtovg::ImageFlags::PREMULTIPLIED | femtovg::ImageFlags::FLIP_Y,
            )
            .unwrap();
        canvas.save();
        canvas.set_render_target(femtovg::RenderTarget::Image(mask));
        canvas.clear_rect(0, 0, W, H, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
        canvas.reset_transform();
        let mut r = Path::new();
        r.rect(0.0, 0.0, W as f32, H as f32);
        let fade = Paint::linear_gradient(
            0.0,
            0.0,
            0.0,
            H as f32,
            Color::white(),
            Color::rgbaf(1.0, 1.0, 1.0, 0.0),
        );
        canvas.fill_path(&r, &fade);
        canvas.set_render_target(femtovg::RenderTarget::Screen);
        canvas.restore();

        canvas.begin_layer(&LayerEffects::new().with_mask(
            mask,
            femtovg::MaskKind::Luminance,
            0.0,
            0.0,
            W as f32,
            H as f32,
        ));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &Paint::color(Color::rgb(255, 0, 0)));
        canvas.end_layer();
    });
    // Top row: coverage ~1 -> red survives.
    assert!(
        close(px(&out, 32, 1)[0], 255) && close(px(&out, 32, 1)[1], 0),
        "top should stay red, got {:?}",
        px(&out, 32, 1)
    );
    // Midpoint: coverage ~0.5 -> half red over white.
    let mid = px(&out, 32, H / 2);
    assert!(
        (mid[1] as i32 - 128).abs() <= 12 && close(mid[0], 255),
        "midpoint should be half-faded red, got {mid:?}"
    );
    // Bottom row: coverage ~0 -> white shows through.
    let bottom = px(&out, 32, H - 1);
    assert!(bottom[1] > 240, "bottom should fade to white, got {bottom:?}");
}
