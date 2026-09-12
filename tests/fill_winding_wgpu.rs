//! Headless GPU tests: a filled contour must cover the same pixels whichever
//! way round it was wound. The antialiasing fringe is extruded along each
//! point's miter vector, whose direction follows the point order, so a
//! clockwise contour used to extrude it outward and land a pixel wide all
//! round (#308) - while `Path::rect()` and `Path::circle()`, which emit
//! counter-clockwise, were exact. Winding still has to mean something, though:
//! it is how a nonzero fill tells a hole from a solid, the way SVG and Canvas
//! write them, so these pin both halves.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, LineCap, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 256;
const H: u32 = 256;

fn fill(device: &wgpu::Device, queue: &wgpu::Queue, rule: FillRule, path: &Path) -> Vec<u8> {
    common::render_rgba(
        device,
        queue,
        W,
        H,
        Color::rgba(0, 0, 0, 0),
        |canvas: &mut Canvas<WGPURenderer>| {
            canvas.fill_path(
                path,
                &Paint::color(Color::rgb(0, 0, 0))
                    .with_anti_alias(true)
                    .with_fill_rule(rule),
            );
        },
    )
}

/// Summed alpha, i.e. the area the fill actually covered, in pixels.
fn covered(px: &[u8]) -> f64 {
    px.chunks_exact(4).map(|c| c[3] as f64 / 255.0).sum()
}

fn alpha_at(px: &[u8], x: usize, y: usize) -> f64 {
    px[(y * W as usize + x) * 4 + 3] as f64 / 255.0
}

fn square(x0: f32, y0: f32, x1: f32, y1: f32, clockwise: bool) -> Vec<(f32, f32)> {
    if clockwise {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    } else {
        vec![(x0, y0), (x0, y1), (x1, y1), (x1, y0)]
    }
}

fn contour(path: &mut Path, points: &[(f32, f32)]) {
    for (i, (x, y)) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(*x, *y);
        } else {
            path.line_to(*x, *y);
        }
    }
    path.close();
}

#[test]
fn clockwise_and_counter_clockwise_fills_cover_the_same_area() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // Edges on exact pixel boundaries, so the true coverage is a whole number
    // and neither winding has any partial pixel to hide an error in.
    for clockwise in [false, true] {
        let mut path = Path::new();
        contour(&mut path, &square(100.0, 100.0, 200.0, 200.0, clockwise));
        let px = fill(&device, &queue, FillRule::NonZero, &path);
        let how = if clockwise { "clockwise" } else { "counter-clockwise" };

        assert!(
            (covered(&px) - 10000.0).abs() < 1.0,
            "{how} 100x100 fill covered {:.2}px, want 10000",
            covered(&px)
        );
        // The left edge is at x=100, so pixel 99 is outside and 100 is inside.
        assert_eq!(
            alpha_at(&px, 99, 150),
            0.0,
            "{how} fill leaked into the pixel left of its edge"
        );
        assert_eq!(alpha_at(&px, 100, 150), 1.0, "{how} fill did not reach its own edge");
    }
}

#[test]
fn winding_still_decides_holes_under_nonzero() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // Outer counter-clockwise; the inner contour's winding is what says whether
    // the middle is a hole. Normalizing orientation for the fringe must not
    // take that away.
    let mut hole = Path::new();
    contour(&mut hole, &square(60.0, 60.0, 196.0, 196.0, false));
    contour(&mut hole, &square(100.0, 100.0, 156.0, 156.0, true));
    let px = fill(&device, &queue, FillRule::NonZero, &hole);
    assert_eq!(alpha_at(&px, 128, 128), 0.0, "opposite winding must punch a hole");
    assert_eq!(alpha_at(&px, 80, 128), 1.0, "the ring around the hole must stay filled");

    let mut solid = Path::new();
    contour(&mut solid, &square(60.0, 60.0, 196.0, 196.0, false));
    contour(&mut solid, &square(100.0, 100.0, 156.0, 156.0, false));
    let px = fill(&device, &queue, FillRule::NonZero, &solid);
    assert_eq!(
        alpha_at(&px, 128, 128),
        1.0,
        "same winding must stay solid under nonzero"
    );
}

#[test]
fn even_odd_fills_match_whichever_way_the_hole_is_wound() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // Even-odd ignores direction by definition, so both windings must produce
    // not just the same hole but the same coverage down to the fringe.
    let mut areas = Vec::new();
    for inner_clockwise in [false, true] {
        let mut path = Path::new();
        contour(&mut path, &square(60.0, 60.0, 196.0, 196.0, false));
        contour(&mut path, &square(100.0, 100.0, 156.0, 156.0, inner_clockwise));
        let px = fill(&device, &queue, FillRule::EvenOdd, &path);
        assert_eq!(alpha_at(&px, 128, 128), 0.0, "even-odd must punch the hole either way");
        areas.push(covered(&px));
    }
    assert!(
        (areas[0] - areas[1]).abs() < 1.0,
        "even-odd coverage differed by winding: {:.2} vs {:.2}",
        areas[0],
        areas[1]
    );
}

#[test]
fn a_reversed_concave_contour_covers_its_own_area() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // Concave, so it goes through the stencil rather than the convex fast path,
    // and the fan's winding is what the stencil counts.
    let l_shape = [
        (100.0f32, 100.0f32),
        (200.0, 100.0),
        (200.0, 150.0),
        (150.0, 150.0),
        (150.0, 200.0),
        (100.0, 200.0),
    ];
    for reversed in [false, true] {
        let mut points = l_shape.to_vec();
        if reversed {
            points.reverse();
        }
        let mut path = Path::new();
        contour(&mut path, &points);
        let px = fill(&device, &queue, FillRule::NonZero, &path);
        assert!(
            (covered(&px) - 7500.0).abs() < 1.0,
            "L shape ({}) covered {:.2}px, want 7500",
            if reversed { "reversed" } else { "as authored" },
            covered(&px)
        );
    }
}

#[test]
fn a_stroke_after_a_fill_still_sees_the_path_as_written() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    // The fill normalizes orientation on the path cache that a later stroke of
    // the same path reads, and a stroke's direction decides which end takes
    // which cap. Stroking on its own and stroking after a fill must agree, for
    // a closed contour and for an open one with different caps at each end.
    let closed = || {
        let mut path = Path::new();
        contour(&mut path, &square(100.0, 100.0, 200.0, 200.0, true));
        path
    };
    let open = || {
        let mut path = Path::new();
        path.move_to(80.0, 80.0);
        path.line_to(200.0, 90.0);
        path.line_to(190.0, 200.0);
        path
    };

    for (what, make) in [
        ("closed clockwise square", Box::new(closed) as Box<dyn Fn() -> Path>),
        ("open clockwise polyline", Box::new(open)),
    ] {
        let stroke = |canvas: &mut Canvas<WGPURenderer>| {
            let mut paint = Paint::color(Color::rgb(0, 0, 0))
                .with_line_width(11.0)
                .with_anti_alias(true);
            paint.set_line_cap_start(LineCap::Round);
            paint.set_line_cap_end(LineCap::Square);
            canvas.stroke_path(&make(), &paint);
        };
        let alone = common::render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |canvas| stroke(canvas));
        let after_fill = common::render_rgba(&device, &queue, W, H, Color::rgba(0, 0, 0, 0), |canvas| {
            canvas.fill_path(&make(), &Paint::color(Color::rgba(0, 0, 0, 0)).with_anti_alias(true));
            stroke(canvas);
        });
        assert_eq!(alone, after_fill, "filling first changed the stroke of a {what}");
    }
}
