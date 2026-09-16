//! Headless GPU tests pinning the layer-mask gaps raised in review of the
//! mask work: a masked layer must survive a mid-layer flush like an unmasked
//! one, a nested layer's mask rect is root device space at any depth, a
//! reset inside a masked layer must return every image the layer held,
//! `LayerEffects::default()` must mean what `LayerEffects::new()` means, a
//! rounded scissor must clip a blurred layer where it was set, and a layer
//! admitted by `begin_layer` must apply the filter it declared. Skips without
//! a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    renderer::WGPURenderer, Canvas, Color, ImageFilter, ImageFlags, ImageId, LayerEffects, MaskKind, Paint, Path,
    PixelFormat, RenderTarget,
};

mod common;
use common::headless_device;

const W: u32 = 64;
const H: u32 = 64;
const IMAGE_BYTES: usize = (W as usize) * (H as usize) * 4;

fn output_texture(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("layer mask review target"),
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

/// A white canvas ready to draw on, and the texture its frames land in.
fn white_canvas(device: &wgpu::Device, queue: &wgpu::Queue) -> (Canvas<WGPURenderer>, wgpu::Texture) {
    let target = output_texture(device);
    let renderer = WGPURenderer::new(device.clone(), queue.clone());
    let mut canvas = Canvas::new(renderer).expect("canvas");
    canvas.set_size(W, H, 1.0);
    canvas.clear_rect(0, 0, W, H, Color::white());
    (canvas, target)
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let (mut canvas, target) = white_canvas(device, queue);
    draw(&mut canvas);
    queue.submit(canvas.flush_to_output(&target));
    readback(device, queue, &target)
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

fn close(a: u8, b: i32) -> bool {
    (a as i32 - b).abs() <= 6
}

fn is_red(c: [u8; 3]) -> bool {
    close(c[0], 255) && close(c[1], 0) && close(c[2], 0)
}

fn fill_rect(canvas: &mut Canvas<WGPURenderer>, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    canvas.fill_path(&p, &Paint::color(color));
}

/// A canvas-sized mask image (render target, FLIP_Y) holding white wherever
/// `white` covers and transparent elsewhere: the way an SVG integration
/// rasterizes `<mask>` content.
fn white_mask(canvas: &mut Canvas<WGPURenderer>, white: (f32, f32, f32, f32)) -> ImageId {
    let mask = canvas
        .create_image_empty(
            W as usize,
            H as usize,
            PixelFormat::Rgba8,
            ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y,
        )
        .unwrap();
    canvas.save();
    canvas.set_render_target(RenderTarget::Image(mask));
    canvas.clear_rect(0, 0, W, H, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
    canvas.reset_transform();
    fill_rect(canvas, white.0, white.1, white.2, white.3, Color::white());
    canvas.set_render_target(RenderTarget::Screen);
    canvas.restore();
    mask
}

/// Effects masking a layer by `image` placed over the whole canvas.
fn full_mask(image: ImageId, kind: MaskKind) -> LayerEffects {
    LayerEffects::new().with_mask(image, kind, 0.0, 0.0, W as f32, H as f32)
}

/// A masked layer open across a flush keeps its mask as well as its capture:
/// the draw before the flush and the draw after both composite through the
/// mask at end_layer.
fn masked_layer_survives_a_flush(device: &wgpu::Device, queue: &wgpu::Queue, kind: MaskKind) {
    let (mut canvas, target) = white_canvas(device, queue);
    // White on the top half: the bottom half of the layer is masked out.
    let mask = white_mask(&mut canvas, (0.0, 0.0, W as f32, 32.0));
    let effects = full_mask(mask, kind);
    assert!(canvas.begin_layer(&effects));
    fill_rect(&mut canvas, 0.0, 0.0, 32.0, H as f32, Color::rgb(255, 0, 0));
    queue.submit(canvas.flush_to_output(&target));
    fill_rect(&mut canvas, 32.0, 0.0, 32.0, H as f32, Color::rgb(0, 0, 255));
    canvas.end_layer();
    queue.submit(canvas.flush_to_output(&target));
    let out = readback(device, queue, &target);
    let red = px(&out, 16, 16);
    let blue = px(&out, 48, 16);
    assert!(
        is_red(red),
        "{kind:?}: the draw before the flush shows through the mask's white half, got {red:?}"
    );
    assert!(
        close(blue[0], 0) && close(blue[1], 0) && close(blue[2], 255),
        "{kind:?}: the draw after the flush shows through the mask's white half, got {blue:?}"
    );
    assert_eq!(
        px(&out, 16, 48),
        [255, 255, 255],
        "{kind:?}: the mask still hides the bottom half of the earlier draw"
    );
    assert_eq!(
        px(&out, 48, 48),
        [255, 255, 255],
        "{kind:?}: the mask still hides the bottom half of the later draw"
    );
}

#[test]
fn a_luminance_masked_layer_survives_a_flush() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    masked_layer_survives_a_flush(&device, &queue, MaskKind::Luminance);
}

#[test]
fn an_alpha_masked_layer_survives_a_flush() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    masked_layer_survives_a_flush(&device, &queue, MaskKind::Alpha);
}

/// A mask rect is root device space however deep the layer is nested: with
/// an outer layer captured from device x = 16 on, a mask covering root
/// x = 16..32 keeps the inner layer's red there and nowhere else.
#[test]
fn a_nested_layers_mask_rect_is_root_device_space() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let mut outcomes = Vec::new();
    for kind in [MaskKind::Alpha, MaskKind::Luminance] {
        let out = render(&device, &queue, |canvas| {
            // White over root x = 16..32, full height.
            let mask = white_mask(canvas, (16.0, 0.0, 16.0, H as f32));
            let effects = full_mask(mask, kind);
            // The outer layer's capture starts at device (16, 0).
            canvas.scissor(16.0, 0.0, 48.0, H as f32);
            assert!(canvas.begin_layer(&LayerEffects::new()));
            assert!(canvas.begin_layer(&effects));
            fill_rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0));
            canvas.end_layer();
            canvas.end_layer();
        });
        outcomes.push((kind, px(&out, 20, 32), px(&out, 40, 32)));
    }
    eprintln!("(mask kind, root x = 20, root x = 40): {outcomes:?}");
    for (kind, inside, outside) in outcomes {
        assert!(
            is_red(inside),
            "{kind:?}: root x = 20 lies under the mask's white band, got {inside:?}"
        );
        assert_eq!(
            outside,
            [255, 255, 255],
            "{kind:?}: root x = 40 lies outside the mask's white band, got {outside:?}"
        );
    }
}

/// Two enclosing captures shift the inner store twice, and the mask's
/// placement backs out both, not the innermost origin alone: with the outer
/// layer captured from root x = 16 and a middle one from root x = 24 (x = 8
/// of the outer store), a mask covering root x = 24..40 keeps the inner
/// layer's red there and nowhere else.
#[test]
fn a_mask_rect_stays_root_device_space_under_two_enclosing_layers() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let mut outcomes = Vec::new();
    for kind in [MaskKind::Alpha, MaskKind::Luminance] {
        let out = render(&device, &queue, |canvas| {
            // White over root x = 24..40, full height.
            let mask = white_mask(canvas, (24.0, 0.0, 16.0, H as f32));
            let effects = full_mask(mask, kind);
            // The outer capture starts at root (16, 0); inside it device
            // space is shifted by -16, so the middle capture starts at
            // root (24, 0), which is (8, 0) of the outer store.
            canvas.scissor(16.0, 0.0, 48.0, H as f32);
            assert!(canvas.begin_layer(&LayerEffects::new()));
            canvas.scissor(24.0, 0.0, 40.0, H as f32);
            assert!(canvas.begin_layer(&LayerEffects::new()));
            assert!(canvas.begin_layer(&effects));
            fill_rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0));
            canvas.end_layer();
            canvas.end_layer();
            canvas.end_layer();
        });
        outcomes.push((kind, px(&out, 28, 32), px(&out, 48, 32)));
    }
    eprintln!("(mask kind, root x = 28, root x = 48): {outcomes:?}");
    for (kind, inside, outside) in outcomes {
        assert!(
            is_red(inside),
            "{kind:?}: root x = 28 lies under the mask's white band, got {inside:?}"
        );
        assert_eq!(
            outside,
            [255, 255, 255],
            "{kind:?}: root x = 48 lies outside the mask's white band, got {outside:?}"
        );
    }
}

/// Under a budget of exactly one masked layer's images, a masked layer that a
/// reset or a resize discards leaves room for the next masked layer: its
/// coverage images went back with its capture.
fn masked_layer_after_discard(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    discard: impl FnOnce(&mut Canvas<WGPURenderer>),
    what: &str,
) {
    let mut admitted = None;
    let out = render(device, queue, |canvas| {
        // Capture, normalized mask, converted mask: what a luminance mask needs.
        canvas.set_transient_image_budget(3 * IMAGE_BYTES);
        let mask = white_mask(canvas, (0.0, 0.0, W as f32, 32.0));
        let effects = full_mask(mask, MaskKind::Luminance);
        assert!(canvas.begin_layer(&effects), "{what}: the first masked layer fits");
        discard(canvas);
        canvas.clear_rect(0, 0, W, H, Color::white());
        admitted = Some(canvas.begin_layer(&effects));
        fill_rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0));
        canvas.end_layer();
    });
    let shown = px(&out, 32, 16);
    let hidden = px(&out, 32, 48);
    eprintln!("{what}: second begin_layer returned {admitted:?}; masked-in half {shown:?}, masked-out half {hidden:?}");
    assert_eq!(
        admitted,
        Some(true),
        "{what}: the discarded layer's images are free for the next masked layer"
    );
    assert!(is_red(shown), "{what}: masked-in half, got {shown:?}");
    assert_eq!(hidden, [255, 255, 255], "{what}: masked-out half, got {hidden:?}");
}

#[test]
fn a_reset_inside_a_masked_layer_frees_its_mask_images() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    masked_layer_after_discard(&device, &queue, |canvas| canvas.reset(), "reset");
}

#[test]
fn a_resize_inside_a_masked_layer_frees_its_mask_images() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    masked_layer_after_discard(
        &device,
        &queue,
        |canvas| {
            canvas.set_size(W + 64, H, 1.0);
            canvas.set_size(W, H, 1.0);
        },
        "resize",
    );
}

/// `LayerEffects::default()` is the no-op effects `LayerEffects::new()`
/// describes: a layer opened with it composites at full opacity.
#[test]
fn default_layer_effects_composite_at_full_opacity() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let masked = render(&device, &queue, |canvas| {
        let mask = white_mask(canvas, (0.0, 0.0, W as f32, H as f32));
        let effects = LayerEffects::default().with_mask(mask, MaskKind::Luminance, 0.0, 0.0, W as f32, H as f32);
        assert!(canvas.begin_layer(&effects));
        fill_rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0));
        canvas.end_layer();
    });
    let c = px(&masked, 32, 32);
    assert!(
        is_red(c),
        "default effects with a full white mask show the layer, got {c:?}"
    );

    let plain = render(&device, &queue, |canvas| {
        assert!(canvas.begin_layer(&LayerEffects::default()));
        fill_rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0));
        canvas.end_layer();
    });
    let c = px(&plain, 32, 32);
    assert!(is_red(c), "default effects alone show the layer, got {c:?}");
}

/// A rounded scissor set before a blurred layer clips the layer where it was
/// set: the blur's padding moves the content into the store, and the
/// scissor must move with it, so both edges of the clip come out alike.
#[test]
fn a_rounded_scissor_clips_a_blurred_layer_where_it_was_set() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let out = render(&device, &queue, |canvas| {
        canvas.rounded_scissor(8.0, 8.0, 48.0, 48.0, 8.0); // device 8..56 each axis
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::GaussianBlur { sigma: 2.0 }])));
        fill_rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0));
        canvas.end_layer();
    });
    // Six pixels (three sigma) inside each edge of the clip, on its middle row.
    let left = px(&out, 14, 32);
    let right = px(&out, 50, 32);
    let profile: Vec<(u32, u8)> = (0..W).step_by(2).map(|x| (x, px(&out, x, 32)[1])).collect();
    eprintln!("middle row, (x, green) - 0 is solid red, 255 white: {profile:?}");
    assert!(is_red(left), "inside the left edge of the clip, got {left:?}");
    assert!(is_red(right), "inside the right edge of the clip, got {right:?}");
    assert_eq!(
        px(&out, 4, 32),
        [255, 255, 255],
        "outside the clip nothing of the layer shows"
    );
}

/// A layer `begin_layer` admits applies every effect it declared: with a
/// white luminance mask and a `brightness(0)` filter, `true` means the
/// composite is black, whatever the transient budget was.
#[test]
fn an_admitted_layer_applies_its_declared_filter() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let mut outcomes = Vec::new();
    for images in [3usize, 4] {
        let mut admitted = None;
        let out = render(&device, &queue, |canvas| {
            canvas.set_transient_image_budget(images * IMAGE_BYTES);
            let mask = white_mask(canvas, (0.0, 0.0, W as f32, H as f32));
            let effects = full_mask(mask, MaskKind::Luminance).with_filters(&[ImageFilter::brightness(0.0)]);
            admitted = Some(canvas.begin_layer(&effects));
            fill_rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0));
            canvas.end_layer();
        });
        outcomes.push((images, admitted.unwrap(), px(&out, 32, 32)));
    }
    eprintln!("(budget in images, begin_layer returned, centre pixel): {outcomes:?}");
    for (images, admitted, centre) in outcomes {
        if admitted {
            assert_eq!(
                centre,
                [0, 0, 0],
                "a {images}-image budget: begin_layer returned true, so brightness(0) applies"
            );
        }
    }
}
