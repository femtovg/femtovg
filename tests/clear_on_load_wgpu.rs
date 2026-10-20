//! A clear of the whole target before anything is drawn into it is the load
//! op of the target's render pass, not a quad drawn over what the pass
//! loaded: on a tiler each tile then starts out as the color instead of
//! being read from memory. The two must write the same bytes on the formats
//! a screen can have, and a clear of part of the target must stay a quad.
//! Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 4;
const H: u32 = 4;

/// A canvas on a `format` screen of `W` x `H`, and the screen.
fn screen(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
) -> (Canvas<WGPURenderer>, wgpu::Texture) {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("clear on load test target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(W, H, 1.0);
    (canvas, target)
}

/// Runs `draw` as a frame on `target` and returns the target's bytes.
fn frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    (canvas, target): &mut (Canvas<WGPURenderer>, wgpu::Texture),
    draw: impl FnOnce(&mut Canvas<WGPURenderer>),
) -> Vec<u8> {
    draw(canvas);
    let commands = canvas.flush_to_output(&*target).expect("a frame with commands");
    let row = W * target.format().block_copy_size(None).unwrap();
    let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
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
    queue.submit([commands, enc.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = slice.get_mapped_range().unwrap();
    (0..H as usize)
        .flat_map(|y| mapped[y * padded as usize..][..row as usize].to_vec())
        .collect()
}

/// Colors whose channels sit on, between and next to 8-bit levels, where a
/// conversion's rounding shows, at alphas that premultiply them.
fn colors() -> Vec<Color> {
    let mut colors = vec![
        Color::rgbaf(0.0, 0.0, 0.0, 0.0),
        Color::white(),
        Color::black(),
        // The examples' background: 0.3 is 76.5 levels in single precision.
        Color::rgbf(0.3, 0.3, 0.32),
        Color::rgbaf(0.3, 0.6, 0.9, 0.5),
        Color::rgba(200, 100, 50, 128),
    ];
    for level in (0..255).step_by(3) {
        let half = (level as f32 + 0.5) / 255.0;
        let below = f32::from_bits(half.to_bits() - 1);
        let above = f32::from_bits(half.to_bits() + 1);
        for alpha in [1.0, 0.5, 0.2, 129.0 / 255.0] {
            colors.push(Color::rgbaf(half, below, above, alpha));
            colors.push(Color::rgbaf(level as f32 / 255.0, 1.0 - half, half * 0.5, alpha));
        }
    }
    colors
}

/// Clears a `format` screen to each of `colors()` on load and with the quad,
/// and returns the bytes of each pair that differ.
fn differing_clears(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
) -> Vec<(Color, Vec<u8>, Vec<u8>)> {
    let (mut on_load, mut quad) = (screen(device, queue, format), screen(device, queue, format));
    colors()
        .into_iter()
        .filter_map(|color| {
            let cleared = frame(device, queue, &mut on_load, |canvas| {
                canvas.clear_rect(0, 0, W, H, color)
            });
            // A draw begins the pass, so the clear after it is the quad; it covers the draw.
            let drawn = frame(device, queue, &mut quad, |canvas| {
                let mut path = Path::new();
                path.rect(0.0, 0.0, 2.0, 2.0);
                canvas.fill_path(&path, &Paint::color(Color::rgb(10, 200, 30)));
                canvas.clear_rect(0, 0, W, H, color);
            });
            (cleared != drawn).then_some((color, cleared, drawn))
        })
        .collect()
}

#[test]
fn a_clear_of_the_whole_target_on_load_writes_what_its_quad_would() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let differing = differing_clears(&device, &queue, format);
        assert!(
            differing.is_empty(),
            "{format:?}: {} of {} clears differ, first {:?}",
            differing.len(),
            colors().len(),
            differing[0]
        );
    }
}

/// Apple GPUs store a shader's output to a half-float target rounded toward
/// zero, and a clear value rounded to nearest: the clear on load is the
/// nearer, by a unit in the last place.
#[test]
fn a_clear_of_a_half_float_target_on_load_is_within_a_unit_of_its_quad() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for (color, on_load, quad) in differing_clears(&device, &queue, wgpu::TextureFormat::Rgba16Float) {
        let halves = |bytes: &[u8]| -> Vec<i32> {
            bytes
                .chunks(2)
                .map(|half| i32::from(u16::from_le_bytes([half[0], half[1]])))
                .collect()
        };
        for (a, b) in halves(&on_load).into_iter().zip(halves(&quad)) {
            assert!((a - b).abs() <= 1, "{color:?}: on load {a:#06x}, quad {b:#06x}");
        }
    }
}

#[test]
fn a_clear_of_part_of_the_target_leaves_the_rest() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let mut screen = screen(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let pixels = frame(&device, &queue, &mut screen, |canvas| {
        canvas.clear_rect(0, 0, W, H, Color::white());
        canvas.clear_rect(0, 0, W, H - 1, Color::rgb(255, 0, 0));
        canvas.clear_rect(1, 0, W - 1, H, Color::rgb(0, 0, 255));
    });
    let px = |x: u32, y: u32| &pixels[((y * W + x) * 4) as usize..][..4];
    assert_eq!(px(0, 0), [255, 0, 0, 255], "the rows the first clear covers");
    assert_eq!(px(0, H - 1), [255, 255, 255, 255], "the row neither covers");
    assert_eq!(px(1, H - 1), [0, 0, 255, 255], "the columns the second clear covers");
}
