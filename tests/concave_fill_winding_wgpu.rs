#![cfg(feature = "wgpu")]
//! A concave fill counts winding in the stencil and zeroes it with its cover.
//! A fringe moves the fill's vertices outward from a hole - and from a contour
//! the fill takes for a hole's boundary - so the winding pass can reach past
//! the cover; what it counts there has to be zeroed too.

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 96;
const H: u32 = 96;

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2]]
}

/// A right triangle a fraction of a pixel thick, its square corner at (x, y):
/// its hole's fill vertices move past the triangle.
fn thin_triangle(x: f32, y: f32) -> Path {
    let t = 0.15;
    let mut ring = Path::new();
    ring.move_to(x, y);
    ring.line_to(x, y + 12.0);
    ring.line_to(x + 12.0, y);
    ring.close();
    ring.move_to(x + t, y + t);
    ring.line_to(x + 12.0 - 2.5 * t, y + t);
    ring.line_to(x + t, y + 12.0 - 2.5 * t);
    ring.close();
    ring
}

/// A kite with a sharp corner at (x, y), and a hole crossing the middle of its
/// first edge, so that the kite is taken for a hole's boundary and its fill
/// vertices move outward - most at the corner, along its miter.
fn kite(x: f32, y: f32) -> Path {
    let mut kite = Path::new();
    kite.move_to(x, y);
    kite.line_to(x + 10.0, y + 3.0);
    kite.line_to(x + 3.0, y + 10.0);
    kite.close();
    kite.move_to(x + 4.5, y + 1.2);
    kite.line_to(x + 5.0, y + 2.5);
    kite.line_to(x + 5.5, y + 1.3);
    kite.close();
    kite
}

/// After such shapes at a spread of sub-pixel positions, a concave frame whose
/// cover takes them all in while its hole leaves them out: none of the frame's
/// colour may land in its hole.
#[test]
fn a_concave_fill_leaves_no_winding_past_its_cover() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render_rgba(
        &device,
        &queue,
        W,
        H,
        Color::white(),
        |canvas: &mut Canvas<WGPURenderer>| {
            // The triangles are thin enough for coverage masks, which draw a
            // fill without the stencil; a thin fill the masks refuse or are
            // off for is stencilled as these are.
            canvas.set_thin_fill_masks(false);
            for i in 0..4 {
                for j in 0..4 {
                    let (dx, dy) = (0.25 * j as f32, 0.25 * i as f32);
                    let (x, y) = (12.0 + 18.0 * i as f32 + dx, 12.0 + 18.0 * j as f32 + dy);
                    let shape = if (i + j) % 2 == 0 {
                        thin_triangle(x, y)
                    } else {
                        kite(x, y)
                    };
                    canvas.fill_path(&shape, &Paint::color(Color::rgb(255, 0, 0)));
                }
            }
            let mut frame = Path::new();
            frame.rect(2.0, 2.0, W as f32 - 4.0, H as f32 - 4.0);
            // Wound against the rect: a hole.
            frame.move_to(8.0, 8.0);
            frame.line_to(W as f32 - 8.0, 8.0);
            frame.line_to(W as f32 - 8.0, H as f32 - 8.0);
            frame.line_to(8.0, H as f32 - 8.0);
            frame.close();
            canvas.fill_path(&frame, &Paint::color(Color::rgb(0, 0, 255)));
        },
    );
    let mut stray = Vec::new();
    for y in 9..H - 9 {
        for x in 9..W - 9 {
            let [r, g, b] = px(&pixels, x, y);
            if b > 200 && r < 60 && g < 60 {
                stray.push((x, y));
            }
        }
    }
    assert!(stray.is_empty(), "the frame painted inside its hole at {stray:?}");
}
