//! Headless GPU tests for layer capture and effects (`Canvas::begin_layer` /
//! `end_layer`): group opacity must fade the layer as one image (no
//! double-blending of overlapping children), filtered layers must not mirror
//! (the FLIP_Y storage-parity bookkeeping), declared blurs must actually
//! spread, nesting must multiply opacities, and the composite must honor the
//! outer scissor. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFilter, LayerEffects, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 64;
const H: u32 = 64;

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

fn readback(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture) -> Vec<u8> {
    let unpadded = W * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
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
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = slice.get_mapped_range().unwrap();
    let mut out = vec![0u8; (W * H * 4) as usize];
    for y in 0..H as usize {
        let s = y * padded as usize;
        let d = y * (W * 4) as usize;
        out[d..d + (W * 4) as usize].copy_from_slice(&mapped[s..s + (W * 4) as usize]);
    }
    out
}

fn output_texture(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("layer flush test target"),
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
    })
}

/// A flush in the middle of an open layer must not release the layer's
/// backing image or lose the redirect into it: draws before and after the
/// flush both belong to the layer and composite with its opacity at end_layer.
#[test]
fn open_layer_survives_a_flush() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let target = output_texture(&device);
    let renderer = WGPURenderer::new(device.clone(), queue.clone());
    let mut canvas = Canvas::new(renderer).expect("canvas");
    canvas.set_size(W, H, 1.0);
    canvas.clear_rect(0, 0, W, H, Color::white());
    canvas.begin_layer(&LayerEffects::new().with_opacity(0.8));
    let mut red = Path::new();
    red.rect(0.0, 0.0, 32.0, 64.0);
    canvas.fill_path(&red, &Paint::color(Color::rgb(255, 0, 0)));
    queue.submit(canvas.flush_to_output(&target));
    let mut blue = Path::new();
    blue.rect(32.0, 0.0, 32.0, 64.0);
    canvas.fill_path(&blue, &Paint::color(Color::rgb(0, 0, 255)));
    canvas.end_layer();
    queue.submit(canvas.flush_to_output(&target));
    let out = readback(&device, &queue, &target);
    // 0.8 red over white = (255, 51, 51); 0.8 blue over white = (51, 51, 255).
    let r = px(&out, 16, 32);
    let b = px(&out, 48, 32);
    assert!(
        close(r[0], 255) && close(r[1], 51) && close(r[2], 51),
        "draw before the flush must survive in the layer, got {r:?}"
    );
    assert!(
        close(b[0], 51) && close(b[1], 51) && close(b[2], 255),
        "draw after the flush must still land in the layer, got {b:?}"
    );
}

/// Past the transient budget a layer degrades to pass-through (its draws
/// still appear, unfaded) instead of allocating, and releasing at flush
/// returns the budget.
#[test]
fn layers_degrade_past_the_transient_budget() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        canvas.set_transient_image_budget(1024); // far below one 64x64 RGBA8 layer
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.5));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &Paint::color(Color::rgb(255, 0, 0)));
        canvas.end_layer();
    });
    let c = px(&out, 32, 32);
    assert!(
        close(c[0], 255) && close(c[1], 0),
        "over budget, the layer passes through and draws unfaded; got {c:?}"
    );
    let out = render(&device, &queue, |canvas| {
        canvas.set_transient_image_budget(64 * 64 * 4); // exactly one layer
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.5));
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&p, &Paint::color(Color::rgb(255, 0, 0)));
        canvas.end_layer();
    });
    let c = px(&out, 32, 32);
    assert!(
        close(c[0], 255) && close(c[1], 128),
        "within budget the layer applies its opacity; got {c:?}"
    );
}

fn black_disc(canvas: &mut Canvas<WGPURenderer>, cx: f32, cy: f32, r: f32) {
    let mut p = Path::new();
    p.circle(cx, cy, r);
    canvas.fill_path(&p, &Paint::color(Color::rgb(0, 0, 0)));
}

/// Two overlapping discs under a half-transparent shadow, cast 20px below.
/// Returns the pixel where both discs' shadows would land (28,44) and one
/// where only the first disc's would (17,44). Neither is under a disc.
fn shadowed_overlap(device: &wgpu::Device, queue: &wgpu::Queue, shadow_set: &str) -> ([u8; 3], [u8; 3]) {
    let buf = render(device, queue, |canvas| {
        let mut bg = Path::new();
        bg.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&bg, &Paint::color(Color::rgb(255, 255, 255)));
        let set_shadow = |c: &mut Canvas<WGPURenderer>| {
            c.set_shadow_color(Color::rgbaf(0.0, 0.0, 0.0, 0.5));
            c.set_shadow_blur(0.0);
            c.set_shadow_offset(0.0, 20.0);
        };
        if shadow_set == "before" {
            set_shadow(canvas);
        }
        canvas.begin_layer(&LayerEffects::new());
        if shadow_set == "inside" {
            set_shadow(canvas);
        }
        black_disc(canvas, 24.0, 24.0, 10.0);
        black_disc(canvas, 32.0, 24.0, 10.0);
        canvas.end_layer();
    });
    (px(&buf, 28, 44), px(&buf, 17, 44))
}

/// A shadow in effect at begin_layer is cast ONCE by the layer's result, the
/// way Canvas 2D's beginLayer applies its shadow to the layer and SVG's
/// feDropShadow applies to a filtered group. Where the two discs' shadows
/// coincide the pixel is the same 50% grey as where only one disc's shadow
/// falls: the layer's coverage is the union, so it cannot darken twice.
/// (Per-draw shadows would give 25% there, and if the shadow state also leaked
/// into the layer's draws the layer would cast a third time, 12.5%.)
#[test]
fn a_layer_casts_its_shadow_once() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (both, one) = shadowed_overlap(&device, &queue, "before");
    assert!(
        close(both[0], 128),
        "overlap of the two shadows: {both:?}, want ~128 (one 50% shadow)"
    );
    assert!(close(one[0], 128), "single shadow: {one:?}, want ~128");
}

/// Inside the layer the shadow state resets, so the children do not each
/// cast their own; setting it again inside is how to shadow individual draws,
/// and then the overlap does compound.
#[test]
fn shadow_set_inside_a_layer_shadows_each_draw() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (both, one) = shadowed_overlap(&device, &queue, "inside");
    assert!(
        close(both[0], 64),
        "two per-draw shadows compounding: {both:?}, want ~64"
    );
    assert!(close(one[0], 128), "single per-draw shadow: {one:?}, want ~128");
}

/// No shadow anywhere: the control for the two above.
#[test]
fn a_layer_without_shadow_state_casts_none() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let (both, one) = shadowed_overlap(&device, &queue, "none");
    assert!(
        close(both[0], 255) && close(one[0], 255),
        "unexpected shadow: {both:?} {one:?}"
    );
}

/// Sibling layers reuse one backing store, and each reuse starts from a
/// cleared image: the second layer's composite carries none of the first
/// layer's content, and a budget that fits a single blurred layer's images
/// renders a frame of forty blurred layers with every blur and opacity
/// applied - what a 1080p portrait of a few hundred layers needs.
#[test]
fn reused_layer_backings_start_clear_and_fit_a_small_budget() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        // One blurred layer's worth: capture, filtered target, chain scratch,
        // each padded by the blur reach (3 * 2 + 2 = 8 px each side).
        let padded = (W as usize + 16) * (H as usize + 16) * 4;
        canvas.set_transient_image_budget(3 * padded);
        canvas.clear_rect(0, 0, W, H, Color::white());

        // First layer: a red rect on the left, faded to 50%.
        canvas.begin_layer(&LayerEffects::new().with_opacity(0.5));
        red_rect(canvas, 0.0, 0.0, 24.0, H as f32);
        canvas.end_layer();

        // Forty more blurred siblings drawing a green rect on the right; each
        // reuses the images of the previous one. If a reused store were not
        // cleared, the red rect would ride along and darken the left side.
        for _ in 0..40 {
            canvas.begin_layer(
                &LayerEffects::new()
                    .with_opacity(0.5)
                    .with_filters(&[ImageFilter::GaussianBlur { sigma: 2.0 }]),
            );
            let mut p = Path::new();
            p.rect(40.0, 0.0, 24.0, H as f32);
            canvas.fill_path(&p, &Paint::color(Color::rgb(0, 255, 0)));
            canvas.end_layer();
        }
    });
    // Left: red at 50% over white, once - not forty-one times.
    let left = px(&out, 12, 32);
    assert!(
        close(left[0], 255) && close(left[1], 128) && close(left[2], 128),
        "left should be the first layer's 50% red only; got {left:?}"
    );
    // Right: forty 50% green layers compound; the centre of the rect converges
    // to green, and the blur softens its edge (a pixel just outside the rect
    // picks up green it would not without the blur).
    let right = px(&out, 52, 32);
    assert!(close(right[0], 0) && close(right[1], 255), "right should converge to green; got {right:?}");
    let edge = px(&out, 37, 32);
    assert!(edge[0] < 250 && edge[1] > 200, "blur should reach outside the rect; got {edge:?}");
    // Between: white, untouched by either layer.
    let gap = px(&out, 32, 32);
    assert!(close(gap[0], 255) && close(gap[1], 255), "gap should stay white; got {gap:?}");
}
