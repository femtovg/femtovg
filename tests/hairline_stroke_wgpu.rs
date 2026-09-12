//! Strokes thinner than a pixel must put down the ink their area calls for.
//!
//! femtovg draws such a stroke at fringe width with its alpha scaled down; a
//! fringe-wide antialiased stroke integrates to one pixel of coverage per unit
//! length, so the scale has to be the width ratio itself. Browsers render
//! sub-pixel strokes as exact coverage (Skia's hairline path scales the paint
//! alpha linearly by the device width), and the nanovg heuristic this replaces
//! squared the ratio, leaving a 0.5 px line at a quarter of its coverage.
#![cfg(feature = "wgpu")]

use femtovg::{Color, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 128;
const H: u32 = 64;

/// Alpha coverage per unit length of a horizontal white stroke of `width` at
/// `y`, summed over the rows the antialiasing can reach and measured away
/// from the caps.
fn ink_per_unit_length(device: &wgpu::Device, queue: &wgpu::Queue, width: f32, y: f32) -> f64 {
    let px = common::render_rgba(device, queue, W, H, Color::rgba(0, 0, 0, 0), |canvas| {
        let mut path = Path::new();
        path.move_to(8.0, y);
        path.line_to(120.0, y);
        let mut paint = Paint::color(Color::rgb(255, 255, 255));
        paint.set_line_width(width);
        paint.set_anti_alias(true);
        canvas.stroke_path(&path, &paint);
    });
    let (x0, x1) = (16u32, 112u32);
    let rows = (y as i32 - 4)..(y as i32 + 5);
    let mut sum = 0.0;
    for row in rows {
        for col in x0..x1 {
            sum += px[((row as u32 * W + col) * 4 + 3) as usize] as f64 / 255.0;
        }
    }
    sum / (x1 - x0) as f64
}

/// Coverage per unit length equals the width, on a pixel boundary and through
/// pixel centres alike, from 0.2 px up through widths the fringe does not
/// touch. Before the fix 0.2 px measured 0.04 and 0.5 px measured 0.25.
#[test]
fn sub_pixel_strokes_cover_their_area() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for width in [0.2f32, 0.4, 0.5, 0.6, 0.8, 1.0, 1.5, 2.0] {
        for y in [32.0f32, 32.5] {
            let ink = ink_per_unit_length(&device, &queue, width, y);
            let want = width as f64;
            assert!(
                (ink - want).abs() <= 0.08 * want + 0.02,
                "width {width} at y={y}: {ink:.3} px of coverage per unit length, expected {want:.3}"
            );
        }
    }
}

/// The ink scales linearly with the width, not quadratically: halving a
/// sub-pixel width halves the coverage.
#[test]
fn sub_pixel_coverage_is_linear_in_width() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let a = ink_per_unit_length(&device, &queue, 0.8, 32.5);
    let b = ink_per_unit_length(&device, &queue, 0.4, 32.5);
    assert!(
        (a / b - 2.0).abs() < 0.25,
        "0.8 px / 0.4 px coverage ratio {:.2}, expected 2",
        a / b
    );
}
