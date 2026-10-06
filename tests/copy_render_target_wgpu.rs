//! Headless GPU tests for `Canvas::copy_render_target`: a copy from the
//! screen in several output formats and from an image target lands upright
//! in a `FLIP_Y` image, at its destination, clipped to both surfaces.
//! Each test draws the copy back next to its source and compares the two.
//! Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{
    renderer::WGPURenderer, Canvas, Color, ImageFlags, ImageId, ImageInfo, Paint, Path, PixelFormat, RenderTarget,
};

mod common;
use common::{headless_device, render_pixels, render_pixels_via};

const W: u32 = 16;
const H: u32 = 16;

type C = Canvas<WGPURenderer>;

fn fill(canvas: &mut C, x: u32, y: u32, w: u32, h: u32, color: Color) {
    let mut path = Path::new();
    path.rect(x as f32, y as f32, w as f32, h as f32);
    let mut paint = Paint::color(color);
    paint.set_anti_alias(false);
    canvas.fill_path(&path, &paint);
}

/// Draws `image` 1:1 at (x, y), without antialiasing.
fn blit(canvas: &mut C, image: ImageId, x: u32, y: u32) {
    let (w, h) = canvas.image_size(image).expect("image");
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let mut path = Path::new();
    path.rect(x, y, w, h);
    let mut paint = Paint::image(image, x, y, w, h, 0.0, 1.0);
    paint.set_anti_alias(false);
    canvas.fill_path(&path, &paint);
}

/// Four differently colored 3x2 quadrants at (x, y): a flip on either axis
/// or swapped red and blue channels shows.
fn quadrants(canvas: &mut C, x: u32, y: u32) {
    fill(canvas, x, y, 3, 2, Color::rgb(255, 0, 0));
    fill(canvas, x + 3, y, 3, 2, Color::rgb(0, 128, 0));
    fill(canvas, x, y + 2, 3, 2, Color::rgb(0, 0, 255));
    fill(canvas, x + 3, y + 2, 3, 2, Color::rgb(128, 128, 128));
}

/// An 8x8 render-target image, cleared to transparent.
fn cleared_image(canvas: &mut C) -> ImageId {
    let image = canvas
        .create_image_empty(8, 8, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y)
        .expect("image");
    canvas.with_render_target(RenderTarget::Image(image), |canvas| {
        canvas.clear_rect(0, 0, 8, 8, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
    });
    image
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// Asserts that the `w` x `h` rect at `copy` holds what the one at `source`
/// does.
fn assert_copied(out: &[u8], source: (u32, u32), copy: (u32, u32), w: u32, h: u32) {
    for y in 0..h {
        for x in 0..w {
            let (want, got) = (px(out, source.0 + x, source.1 + y), px(out, copy.0 + x, copy.1 + y));
            let worst = (0..4).map(|i| (got[i] as i32 - want[i] as i32).abs()).max().unwrap();
            assert!(worst <= 1, "pixel ({x}, {y}) of the copy: {got:?}, expected {want:?}");
        }
    }
}

fn assert_transparent(out: &[u8], x: u32, y: u32, w: u32, h: u32) {
    for py in y..y + h {
        for px_ in x..x + w {
            assert_eq!(px(out, px_, py), [0, 0, 0, 0], "pixel ({px_}, {py}) outside the copy");
        }
    }
}

/// Copies the quadrants from the screen into an image at (1, 2), and draws
/// the image at (8, 8). The screen is a `format` texture seen through a view
/// of `view_format`, on layer 0 of `layers` layers.
fn round_trip(format: wgpu::TextureFormat, view_format: Option<wgpu::TextureFormat>, layers: u32) -> Option<Vec<u8>> {
    let (device, queue) = headless_device()?;
    Some(render_pixels_via(
        &device,
        &queue,
        W,
        H,
        format,
        view_format,
        layers,
        Color::rgbaf(0.0, 0.0, 0.0, 0.0),
        |c| {
            let image = cleared_image(c);
            quadrants(c, 2, 1);
            c.copy_render_target(2, 1, 6, 4, image, 1, 2).expect("copy");
            blit(c, image, 8, 8);
        },
    ))
}

fn assert_round_trip(out: &[u8]) {
    assert_copied(out, (2, 1), (9, 10), 6, 4);
    // The rest of the image keeps what it held.
    assert_transparent(out, 8, 8, 8, 2);
    assert_transparent(out, 8, 14, 8, 2);
    assert_transparent(out, 8, 10, 1, 4);
    assert_transparent(out, 15, 10, 1, 4);
}

fn screen_round_trip(format: wgpu::TextureFormat) {
    if let Some(out) = round_trip(format, None, 1) {
        assert_round_trip(&out);
    }
}

#[test]
fn copies_the_screen_upright() {
    screen_round_trip(wgpu::TextureFormat::Rgba8Unorm);
}

/// The surface format many platforms prefer: its channels arrive swizzled.
#[test]
fn copies_a_bgra_screen() {
    screen_round_trip(wgpu::TextureFormat::Bgra8Unorm);
}

/// Draws onto an sRGB view are encoded, so the copy decodes: the gray
/// quadrant would come back lighter if it were encoded twice.
#[test]
fn copies_an_srgb_screen() {
    screen_round_trip(wgpu::TextureFormat::Rgba8UnormSrgb);
}

/// A non-sRGB texture drawn through an sRGB view: the draws are encoded, so
/// the copy decodes the texture's bytes.
#[test]
fn copies_an_srgb_view_of_a_linear_screen() {
    if let Some(out) = round_trip(
        wgpu::TextureFormat::Rgba8Unorm,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        1,
    ) {
        assert_round_trip(&out);
    }
}

/// An sRGB texture drawn through a linear view: nothing is encoded, so the
/// copy reads the texture's bytes as they are.
#[test]
fn copies_a_linear_view_of_an_srgb_screen() {
    if let Some(out) = round_trip(
        wgpu::TextureFormat::Rgba8UnormSrgb,
        Some(wgpu::TextureFormat::Rgba8Unorm),
        1,
    ) {
        assert_round_trip(&out);
    }
}

/// The output's layer can't be told from its view, so an array texture's
/// screen is skipped rather than read from layer 0.
#[test]
fn skips_a_screen_in_an_array_texture() {
    let Some(out) = round_trip(wgpu::TextureFormat::Rgba8Unorm, None, 2) else {
        return;
    };
    assert_transparent(&out, 8, 8, 8, 8);
}

#[test]
fn copies_an_image_target_upright() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_pixels(
        &device,
        &queue,
        W,
        H,
        wgpu::TextureFormat::Rgba8Unorm,
        Color::rgbaf(0.0, 0.0, 0.0, 0.0),
        |c| {
            let source = cleared_image(c);
            let copy = cleared_image(c);
            c.with_render_target(RenderTarget::Image(source), |c| {
                quadrants(c, 2, 1);
                c.copy_render_target(2, 1, 6, 4, copy, 1, 2).expect("copy");
            });
            blit(c, source, 0, 0);
            blit(c, copy, 8, 8);
        },
    );
    assert_copied(&out, (2, 1), (9, 10), 6, 4);
    assert_transparent(&out, 8, 8, 8, 2);
}

/// A rect reaching past the screen or the image copies the part both hold.
#[test]
fn clips_the_rect_to_both_surfaces() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_pixels(
        &device,
        &queue,
        W,
        H,
        wgpu::TextureFormat::Rgba8Unorm,
        Color::rgbaf(0.0, 0.0, 0.0, 0.0),
        |c| {
            let past_screen = cleared_image(c);
            let past_image = cleared_image(c);
            quadrants(c, 10, 12);
            // 6x4 of the 8x8 rect lie on the screen.
            c.copy_render_target(10, 12, 8, 8, past_screen, 0, 0).expect("copy");
            // 2x2 of the 6x4 rect fit the image.
            c.copy_render_target(10, 12, 6, 4, past_image, 6, 6).expect("copy");
            blit(c, past_screen, 0, 0);
            blit(c, past_image, 0, 8);
        },
    );
    assert_copied(&out, (10, 12), (0, 0), 6, 4);
    assert_transparent(&out, 6, 0, 2, 8);
    assert_transparent(&out, 0, 4, 6, 4);
    assert_copied(&out, (10, 12), (6, 14), 2, 2);
    assert_transparent(&out, 0, 8, 8, 6);
}

/// A native texture made for sampling only can't be drawn into, so a copy
/// into it is skipped instead of failing validation.
#[test]
fn skips_a_sample_only_native_texture() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let out = render_pixels(
        &device,
        &queue,
        W,
        H,
        wgpu::TextureFormat::Rgba8Unorm,
        Color::rgbaf(0.0, 0.0, 0.0, 0.0),
        |c| {
            let info = ImageInfo::new(ImageFlags::PREMULTIPLIED, 8, 8, PixelFormat::Rgba8);
            let image = c.create_image_from_native_texture(texture, info).expect("image");
            quadrants(c, 2, 1);
            c.copy_render_target(2, 1, 6, 4, image, 1, 2).expect("copy");
            blit(c, image, 8, 8);
        },
    );
    assert_transparent(&out, 8, 8, 8, 8);
}
