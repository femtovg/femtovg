//! A rect blur reads its render target outside a render pass: the screen
//! through a copy recorded on the encoder, an image target where it is. The
//! WGPU backend begins a target's pass at its first draw, with a clear of the
//! whole target folded into its load op, and plans each pass's stencil up to
//! the commands that end it, a rect blur's first draw among them. So a clear
//! the pass has not run yet has to run before the blur reads the target, and
//! the pass has to end at the blur whether it runs or is skipped.
//! Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFlags, ImageId, Paint, Path, PixelFormat, RenderTarget};

mod common;
use common::{headless_device, render_frames};

const W: u32 = 64;
const H: u32 = 48;

fn rect(canvas: &mut Canvas<WGPURenderer>, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut path = Path::new();
    path.rect(x, y, w, h);
    let mut paint = Paint::color(color);
    paint.set_anti_alias(false);
    canvas.fill_path(&path, &paint);
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// The three frames of a target that was red: cleared and blurred at once,
/// and then, as a check that the blur runs at all here, cleared, half
/// blackened and blurred across the half's edge at x = 32.
fn blur_frames(canvas: &mut Canvas<WGPURenderer>, frame: usize) {
    match frame {
        0 => rect(canvas, 0.0, 0.0, W as f32, H as f32, Color::rgb(255, 0, 0)),
        1 => canvas.blur_rect(16, 12, 32, 24, 4.0).expect("blur_rect"),
        _ => {
            rect(canvas, 0.0, 0.0, 32.0, H as f32, Color::black());
            canvas.blur_rect(16, 12, 32, 24, 4.0).expect("blur_rect");
        }
    }
}

fn assert_blur_read_the_clear(frames: &[Vec<u8>], target: &str) {
    assert_eq!(
        px(&frames[1], 32, 24),
        [255, 255, 255],
        "{target}: the blur after the clear read the frame before"
    );
    let [edge, ..] = px(&frames[2], 32, 24);
    assert!(
        (60..=200).contains(&edge),
        "{target}: the edge at x = 32 should be blurred to grey, is {edge}"
    );
}

/// A frame that clears the whole target and then blurs a rect of it reads
/// the clear, on the screen and on an image target.
#[test]
fn a_blur_after_a_clear_on_load_reads_the_clear() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let frames = render_frames(&device, &queue, W, H, Color::white(), 3, blur_frames);
    assert_blur_read_the_clear(&frames, "screen");

    let mut image: Option<ImageId> = None;
    let frames = render_frames(&device, &queue, W, H, Color::black(), 3, |canvas, frame| {
        let image = *image.get_or_insert_with(|| {
            canvas
                .create_image_empty(
                    W as usize,
                    H as usize,
                    PixelFormat::Rgba8,
                    ImageFlags::FLIP_Y | ImageFlags::PREMULTIPLIED,
                )
                .expect("image")
        });
        canvas.with_render_target(RenderTarget::Image(image), |canvas| {
            canvas.clear_rect(0, 0, W, H, Color::white());
            blur_frames(canvas, frame);
        });
        let mut path = Path::new();
        path.rect(0.0, 0.0, W as f32, H as f32);
        let mut paint = Paint::image(image, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0);
        paint.set_anti_alias(false);
        canvas.fill_path(&path, &paint);
    });
    assert_blur_read_the_clear(&frames, "image target");
}

/// A pass begun by a fill that tests no stencil, then a blur that is
/// skipped - its rect lies beyond the output, the canvas being wider - and
/// then an L, a concave fill that counts winding in the stencil: the L is
/// filled as an L, not as the box around it.
#[test]
fn a_skipped_blur_ends_the_pass_as_planned() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let frames = render_frames(&device, &queue, W, H, Color::white(), 1, |canvas, _| {
        canvas.set_size(2 * W, H, 1.0);
        rect(canvas, 2.0, 2.0, 4.0, 4.0, Color::rgb(0, 0, 255));
        canvas.blur_rect(W + 8, 8, 16, 16, 2.0).expect("blur_rect");
        let mut l = Path::new();
        l.move_to(16.0, 8.0);
        l.line_to(24.0, 8.0);
        l.line_to(24.0, 32.0);
        l.line_to(48.0, 32.0);
        l.line_to(48.0, 40.0);
        l.line_to(16.0, 40.0);
        l.close();
        let mut paint = Paint::color(Color::rgb(255, 0, 0));
        paint.set_anti_alias(false);
        canvas.fill_path(&l, &paint);
    });
    assert_eq!(px(&frames[0], 20, 20), [255, 0, 0], "the L's upright");
    assert_eq!(px(&frames[0], 36, 20), [255, 255, 255], "the L's notch is filled");
}
