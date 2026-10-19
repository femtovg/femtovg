//! A vertex repeated in a path draws exactly as if it appeared once.
//!
//! A repeat within the distance tolerance is a zero-length edge, and a
//! zero-length edge has no direction: the join at it extrudes along one
//! neighbour's normal only, so a stroke's outer corner is left uncovered and
//! the fill's fringe bulges. nanovg merges the repeat into the point before
//! it as the point is added, and so does femtovg.
#![cfg(feature = "wgpu")]

use femtovg::{Color, LineCap, LineJoin, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 32;
const H: u32 = 32;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, path: &Path, paint: &Paint, stroke: bool) -> Vec<u8> {
    common::render_rgba(device, queue, W, H, Color::rgba(0, 0, 0, 0), |canvas| {
        if stroke {
            canvas.stroke_path(path, paint);
        } else {
            canvas.fill_path(path, paint);
        }
    })
}

fn alpha(px: &[u8], x: u32, y: u32) -> u8 {
    px[((y * W + x) * 4 + 3) as usize]
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count()
}

/// The corner (8,8) -> (18,8) -> (18,18), with the corner given `repeats`
/// extra times.
fn corner(repeats: usize) -> Path {
    let mut path = Path::new();
    path.move_to(8.0, 8.0);
    for _ in 0..=repeats {
        path.line_to(18.0, 8.0);
    }
    path.line_to(18.0, 18.0);
    path
}

/// A width-4 mitred stroke covers the 2 px square outside its corner, at
/// (18..20, 6..8). With the corner repeated, master left that square bare.
#[test]
fn a_repeated_corner_keeps_its_miter() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let mut paint = Paint::color(Color::rgb(255, 255, 255));
    paint.set_line_width(4.0);
    paint.set_line_join(LineJoin::Miter);
    paint.set_anti_alias(true);

    let px = render(&device, &queue, &corner(1), &paint, true);
    for (x, y) in [(18, 6), (19, 6), (18, 7), (19, 7)] {
        assert_eq!(alpha(&px, x, y), 255, "corner pixel ({x}, {y}) is not covered");
    }
}

/// Every join strokes the same pixels whether the corner is given once or
/// three times. Master differed in the mitre's corner square and in three
/// pixels of the bevel.
#[test]
fn a_repeated_corner_strokes_like_a_single_one() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for join in [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel] {
        let mut paint = Paint::color(Color::rgb(255, 255, 255));
        paint.set_line_width(4.0);
        paint.set_line_join(join);
        paint.set_anti_alias(true);

        let once = render(&device, &queue, &corner(0), &paint, true);
        let repeated = render(&device, &queue, &corner(2), &paint, true);
        let moved = differing(&once, &repeated);
        assert_eq!(moved, 0, "{join:?}: {moved} pixels differ");
    }
}

/// The triangle closing the corner fills the same pixels with the corner
/// repeated. Its two legs lie on pixel boundaries, so nothing outside them is
/// inked; master slanted the fringe of both legs at the repeat, inking up to
/// 100/255 outside them and leaving 149/255 inside.
#[test]
fn a_repeated_corner_fills_like_a_single_one() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let mut paint = Paint::color(Color::rgb(255, 255, 255));
    paint.set_anti_alias(true);
    let [once, repeated] = [0, 2].map(|repeats| {
        let mut path = corner(repeats);
        path.close();
        render(&device, &queue, &path, &paint, false)
    });
    let moved = differing(&once, &repeated);
    assert_eq!(moved, 0, "{moved} pixels differ");
    for x in 0..W {
        assert_eq!(alpha(&repeated, x, 7), 0, "({x}, 7) above the top leg is inked");
    }
}

/// An open line whose first and last points are repeated keeps both caps.
/// A cap takes its direction from the edge next to it, which the repeat made
/// zero-length: master lost the square and round caps (36 and 32 pixels).
#[test]
fn repeated_ends_keep_their_caps() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
        let mut paint = Paint::color(Color::rgb(255, 255, 255));
        paint.set_line_width(6.0);
        paint.set_line_cap(cap);
        paint.set_anti_alias(true);
        let [once, repeated] = [0, 1].map(|repeats| {
            let mut path = Path::new();
            path.move_to(8.0, 16.0);
            for _ in 0..repeats {
                path.line_to(8.0, 16.0);
            }
            path.line_to(24.0, 16.0);
            for _ in 0..repeats {
                path.line_to(24.0, 16.0);
            }
            render(&device, &queue, &path, &paint, true)
        });
        let moved = differing(&once, &repeated);
        assert_eq!(moved, 0, "{cap:?}: {moved} pixels differ");
    }
}
