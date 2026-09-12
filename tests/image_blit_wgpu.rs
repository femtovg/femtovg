//! Headless GPU test for the unclipped image-blit fast path a non-antialiased
//! rect fill with an image paint takes (`ShaderType::TextureCopyUnclipped`):
//! under a mirrored transform the rect's corners arrive in the opposite
//! order, which used to read as a negative extent and draw nothing at all -
//! while the same fill with antialiasing, which takes the general path, drew
//! the mirrored image. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFlags, Paint, Path, Transform2D};

mod common;
use common::headless_device;

const W: u32 = 64;
const H: u32 = 64;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("image blit test target"),
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
    queue.submit(canvas.flush_to_output(&target));

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

/// A red-over-transparent image (red on its top half) filled as a rect under
/// y' = H - y must come out red on the BOTTOM half, antialiased or not.
#[test]
fn a_rect_image_fill_draws_mirrored_under_a_flipped_transform() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    for anti_alias in [true, false] {
        let out = render(&device, &queue, |canvas| {
            let mut buf = vec![femtovg::rgb::RGBA8::new(0, 0, 0, 0); (W * H) as usize];
            for p in buf.iter_mut().take((W * H / 2) as usize) {
                *p = femtovg::rgb::RGBA8::new(255, 0, 0, 255);
            }
            let image = canvas
                .create_image(
                    femtovg::imgref::Img::new(buf.as_slice(), W as usize, H as usize),
                    ImageFlags::empty(),
                )
                .unwrap();
            canvas.set_transform(&Transform2D::new(1.0, 0.0, 0.0, -1.0, 0.0, H as f32));
            let mut rect = Path::new();
            rect.rect(0.0, 0.0, W as f32, H as f32);
            let mut paint = Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0);
            paint.set_anti_alias(anti_alias);
            canvas.fill_path(&rect, &paint);
        });
        let top = px(&out, 32, 12);
        let bottom = px(&out, 32, 52);
        assert_eq!(
            top,
            [255, 255, 255],
            "anti_alias={anti_alias}: the image's red top lands at the bottom, got {top:?}"
        );
        assert_eq!(
            bottom,
            [255, 0, 0],
            "anti_alias={anti_alias}: mirrored red expected at the bottom, got {bottom:?} (white means the fill was dropped)"
        );
    }
}
