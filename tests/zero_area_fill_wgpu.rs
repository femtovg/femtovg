//! Headless GPU tests: a fill of a contour whose points lie on one line
//! draws nothing (femtovg/femtovg#341). A bare `<line>` or an open path
//! under SVG's default black fill used to ink a two-pixel line: the
//! antialiasing fringe was extruded on both sides of a contour with no
//! interior. Shapes that enclose area keep drawing, including a small one
//! on a long retraced tail. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 128;
const H: u32 = 128;

/// Fills the path under a uniform `scale` and returns the pixels.
fn fill(device: &wgpu::Device, queue: &wgpu::Queue, scale: f32, build: impl FnOnce(&mut Path)) -> Vec<u8> {
    common::render_rgba(
        device,
        queue,
        W,
        H,
        Color::rgba(0, 0, 0, 0),
        |canvas: &mut Canvas<WGPURenderer>| {
            canvas.scale(scale, scale);
            let mut path = Path::new();
            build(&mut path);
            let mut paint = Paint::color(Color::black());
            paint.set_anti_alias(true);
            canvas.fill_path(&path, &paint);
        },
    )
}

fn ink(px: &[u8]) -> f64 {
    px.chunks_exact(4).map(|c| c[3] as f64 / 255.0).sum()
}

fn alpha(px: &[u8], x: u32, y: u32) -> f64 {
    px[((y * W + x) * 4 + 3) as usize] as f64 / 255.0
}

const SCALES: [f32; 6] = [1.0, 2.0, 4.0, 8.0, 16.0, 64.0];

/// A named path builder.
type Case = (&'static str, fn(&mut Path));

/// Collinear contours, in a 2 x 2 unit box so every zoom keeps them on the
/// canvas, draw nothing at any zoom.
#[test]
fn a_collinear_contour_fills_nothing_at_any_zoom() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let cases: [Case; 4] = [
        ("a bare line, closed", |p| {
            p.move_to(0.2, 0.2);
            p.line_to(0.2, 1.8);
            p.close();
        }),
        ("a bare line, open", |p| {
            p.move_to(0.5, 0.2);
            p.line_to(0.5, 1.8);
        }),
        ("three collinear points", |p| {
            p.move_to(0.8, 0.2);
            p.line_to(0.8, 1.0);
            p.line_to(0.8, 1.8);
            p.close();
        }),
        ("a diagonal with fractional coordinates", |p| {
            p.move_to(0.12345, 0.17891);
            p.line_to(1.67891, 1.95123);
            p.line_to(0.90118, 1.06507);
            p.close();
        }),
    ];
    for scale in SCALES {
        for (name, build) in cases {
            let ink = ink(&fill(&device, &queue, scale, build));
            assert!(ink < 0.05, "{name} at {scale}x: {ink:.2} px of ink, expected none");
        }
    }
}

/// A unit square at the end of a 1000-unit retraced tail encloses one unit
/// of area, and keeps it at every zoom: the classifier looks for a line
/// through all the points, not at area against perimeter.
#[test]
fn a_square_on_a_retraced_tail_still_fills() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for scale in SCALES {
        let px = fill(&device, &queue, scale, |p| {
            p.move_to(0.5, 0.5);
            p.line_to(1000.5, 0.5);
            p.line_to(0.5, 0.5);
            p.line_to(0.5, 1.5);
            p.line_to(1.5, 1.5);
            p.line_to(1.5, 0.5);
            p.close();
        });
        let centre = alpha(&px, scale as u32, scale as u32);
        assert!(centre >= 0.5, "the square's centre at {scale}x: alpha {centre:.2}");
    }
}

/// A sliver a third of a pixel wide encloses area and keeps drawing; a
/// shape wider than the fringe is unaffected.
#[test]
fn shapes_that_enclose_area_still_fill() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let sliver = ink(&fill(&device, &queue, 1.0, |p| {
        p.move_to(20.0, 20.0);
        p.line_to(100.0, 60.0);
        p.line_to(100.3, 60.0);
        p.line_to(20.3, 20.0);
        p.close();
    }));
    assert!(sliver > 5.0, "the sliver draws: {sliver:.2} px of ink");
    let square = ink(&fill(&device, &queue, 1.0, |p| p.rect(20.0, 20.0, 40.0, 40.0)));
    assert!((square - 1600.0).abs() < 1.0, "{square:.2} px, expected 1600");
}

/// Fills the path under `rule` at scale 1 and returns the pixels.
fn fill_with_rule(device: &wgpu::Device, queue: &wgpu::Queue, rule: FillRule, build: impl Fn(&mut Path)) -> Vec<u8> {
    common::render_rgba(
        device,
        queue,
        W,
        H,
        Color::rgba(0, 0, 0, 0),
        |canvas: &mut Canvas<WGPURenderer>| {
            let mut path = Path::new();
            build(&mut path);
            let mut paint = Paint::color(Color::black());
            paint.set_anti_alias(true);
            paint.set_fill_rule(rule);
            canvas.fill_path(&path, &paint);
        },
    )
}

/// A degenerate contour stays out of the hole classification as well: a
/// 0.001 px strip along a hole's first edge used to wind once around the
/// point that decides which way the hole's fringe extrudes, turning the
/// hole inside out - a pixel smaller all round - under either fill rule.
/// Browsers ink nothing for the strip; the hole renders as without it.
#[test]
fn a_degenerate_strip_does_not_reclassify_a_hole() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let square_with_hole = |p: &mut Path| {
        p.move_to(20.0, 20.0);
        p.line_to(108.0, 20.0);
        p.line_to(108.0, 108.0);
        p.line_to(20.0, 108.0);
        p.close();
        // The hole, wound the other way; its first edge runs down x = 40.
        p.move_to(40.0, 40.0);
        p.line_to(40.0, 88.0);
        p.line_to(88.0, 88.0);
        p.line_to(88.0, 40.0);
        p.close();
    };
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let plain = fill_with_rule(&device, &queue, rule, square_with_hole);
        let with_strip = fill_with_rule(&device, &queue, rule, |p| {
            square_with_hole(p);
            p.move_to(39.9995, 40.0);
            p.line_to(40.0005, 40.0);
            p.line_to(40.0005, 88.0);
            p.line_to(39.9995, 88.0);
            p.close();
        });
        assert_eq!(alpha(&plain, 64, 64), 0.0, "{rule:?}: the hole is empty");
        assert_eq!(alpha(&plain, 30, 64), 1.0, "{rule:?}: the ring is inked");
        assert!(plain == with_strip, "{rule:?}: the strip changed the fill");
    }
}
