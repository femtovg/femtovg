//! Headless GPU tests for exact-coverage fills (femtovg/femtovg#327): every
//! pixel of an antialiased fill carries the area the path covers in it, so
//! sub-pixel shapes are as faint as they are thin, holes and even-odd keep
//! their winding, and the fill still honours clips, composite operations,
//! layers and scissors. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, CompositeOperation, FillRule, LayerEffects, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 64;
const H: u32 = 64;

type C = Canvas<WGPURenderer>;
type Poly = Vec<(f64, f64)>;

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut C)) -> Vec<u8> {
    common::render_rgba(device, queue, W, H, Color::rgba(0, 0, 0, 0), draw)
}

fn alpha(px: &[u8], x: u32, y: u32) -> f64 {
    px[((y * W + x) * 4 + 3) as usize] as f64 / 255.0
}

fn ink(px: &[u8]) -> f64 {
    px.chunks_exact(4).map(|c| c[3] as f64 / 255.0).sum()
}

fn path_of(polys: &[Poly]) -> Path {
    let mut path = Path::new();
    for poly in polys {
        for (i, (x, y)) in poly.iter().enumerate() {
            if i == 0 {
                path.move_to(*x as f32, *y as f32);
            } else {
                path.line_to(*x as f32, *y as f32);
            }
        }
        path.close();
    }
    path
}

fn fill_polys(c: &mut C, polys: &[Poly], rule: FillRule) {
    let mut paint = Paint::color(Color::black());
    paint.set_anti_alias(true);
    paint.set_fill_rule(rule);
    c.fill_path(&path_of(polys), &paint);
}

fn signed_area(poly: &[(f64, f64)]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % n];
            x0 * y1 - x1 * y0
        })
        .sum::<f64>()
        * 0.5
}

/// The signed area of `poly` inside the pixel at (px, py), by clipping the
/// polygon against the pixel's four sides.
fn area_in_pixel(poly: &[(f64, f64)], px: f64, py: f64) -> f64 {
    let mut out: Poly = poly.to_vec();
    for (axis, bound, keep_below) in [(0, px, false), (0, px + 1.0, true), (1, py, false), (1, py + 1.0, true)] {
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        let coord = |p: (f64, f64)| if axis == 0 { p.0 } else { p.1 };
        let inside = |p: (f64, f64)| {
            if keep_below {
                coord(p) <= bound
            } else {
                coord(p) >= bound
            }
        };
        let cross = |a: (f64, f64), b: (f64, f64)| {
            let t = (bound - coord(a)) / (coord(b) - coord(a));
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        for i in 0..input.len() {
            let cur = input[i];
            let prev = input[(i + input.len() - 1) % input.len()];
            if inside(cur) {
                if !inside(prev) {
                    out.push(cross(prev, cur));
                }
                out.push(cur);
            } else if inside(prev) {
                out.push(cross(prev, cur));
            }
        }
    }
    signed_area(&out)
}

/// Exact coverage of a pixel under the fill rule: nonzero clamps the
/// winding-weighted area, even-odd folds it.
fn exact_coverage(polys: &[Poly], rule: FillRule, x: u32, y: u32) -> f64 {
    let winding: f64 = polys.iter().map(|p| area_in_pixel(p, x as f64, y as f64)).sum();
    match rule {
        FillRule::NonZero => winding.abs().min(1.0),
        FillRule::EvenOdd => {
            let folded = winding.abs() % 2.0;
            1.0 - (1.0 - folded).abs()
        }
    }
}

fn bar(x0: f64, y0: f64, x1: f64, y1: f64, width: f64) -> Poly {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = (dx * dx + dy * dy).sqrt();
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    vec![
        (x0 + nx, y0 + ny),
        (x1 + nx, y1 + ny),
        (x1 - nx, y1 - ny),
        (x0 - nx, y0 - ny),
    ]
}

fn shapes() -> Vec<(&'static str, Vec<Poly>, FillRule)> {
    let outer: Poly = vec![(10.0, 10.0), (10.0, 50.3), (50.7, 50.3), (50.7, 10.0)];
    let hole_cw: Poly = vec![(20.2, 20.6), (40.1, 20.6), (40.1, 39.5), (20.2, 39.5)];
    let hole_ccw: Poly = hole_cw.iter().rev().copied().collect();
    vec![
        (
            "half-pixel bar at 45 degrees",
            vec![bar(8.0, 8.0, 56.0, 56.0, 0.5)],
            FillRule::NonZero,
        ),
        (
            "an L",
            vec![vec![
                (5.5, 5.5),
                (5.5, 58.2),
                (58.3, 58.2),
                (58.3, 40.7),
                (22.1, 40.7),
                (22.1, 5.5),
            ]],
            FillRule::NonZero,
        ),
        (
            "a triangle",
            vec![vec![(3.3, 60.1), (31.7, 2.4), (61.9, 55.5)]],
            FillRule::NonZero,
        ),
        (
            "a ring, nonzero",
            vec![outer.clone(), hole_cw.clone()],
            FillRule::NonZero,
        ),
        (
            "a ring, even-odd, hole wound the same way",
            vec![outer.clone(), hole_ccw],
            FillRule::EvenOdd,
        ),
        (
            "a ring, even-odd, hole wound the other way",
            vec![outer, hole_cw],
            FillRule::EvenOdd,
        ),
        (
            "a triangle reaching past the canvas",
            vec![vec![(-20.0, 30.0), (40.0, -15.0), (80.0, 70.0)]],
            FillRule::NonZero,
        ),
    ]
}

/// Every pixel within 2/255 of the area the path covers in it, and the
/// total within half a pixel.
#[test]
fn every_pixel_carries_the_exact_area() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for (name, polys, rule) in shapes() {
        let px = render(&device, &queue, |c| fill_polys(c, &polys, rule));
        let mut worst = 0.0f64;
        let mut want_total = 0.0;
        for y in 0..H {
            for x in 0..W {
                let want = exact_coverage(&polys, rule, x, y);
                want_total += want;
                worst = worst.max((alpha(&px, x, y) - want).abs());
            }
        }
        assert!(
            worst <= 2.0 / 255.0,
            "{name}: worst pixel off by {:.1}/255",
            worst * 255.0
        );
        assert!(
            (ink(&px) - want_total).abs() <= 0.5,
            "{name}: {:.2} px of ink, expected {want_total:.2}",
            ink(&px)
        );
    }
}

/// A bar's ink equals its area whatever its width: the sub-pixel widths
/// the fringe used to over-ink (2.4x at a quarter pixel) and the wide ones
/// it already got right.
#[test]
fn bars_of_every_width_carry_their_area() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    for angle in [45.0f64, 30.0] {
        for width in [0.25f64, 0.5, 1.0, 2.0, 4.0] {
            let (dx, dy) = (angle.to_radians().cos() * 40.0, angle.to_radians().sin() * 40.0);
            let polys = vec![bar(12.0, 12.0, 12.0 + dx, 12.0 + dy, width)];
            let px = render(&device, &queue, |c| fill_polys(c, &polys, FillRule::NonZero));
            let want = 40.0 * width;
            let got = ink(&px);
            assert!(
                (got / want - 1.0).abs() <= 0.03,
                "{width} px bar at {angle} degrees: {got:.2} px of ink for {want:.2} of area"
            );
        }
    }
}

/// The coverage fill is a draw like any other: a stencil clip and the
/// scissor bound it, a composite operation applies with its coverage, and
/// it lands in an open layer.
#[test]
fn coverage_fills_honour_clip_scissor_composite_and_layers() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let l_shape: Vec<Poly> = vec![vec![
        (4.0, 4.0),
        (4.0, 60.0),
        (60.0, 60.0),
        (60.0, 44.0),
        (20.0, 44.0),
        (20.0, 4.0),
    ]];

    let clipped = render(&device, &queue, |c| {
        let mut clip = Path::new();
        clip.circle(32.0, 32.0, 20.0);
        c.clip_path(&clip, FillRule::NonZero);
        fill_polys(c, &l_shape, FillRule::NonZero);
    });
    assert_eq!(alpha(&clipped, 6, 6), 0.0, "outside the clip");
    assert_eq!(alpha(&clipped, 14, 32), 1.0, "inside the clip and the shape");

    let scissored = render(&device, &queue, |c| {
        c.scissor(0.0, 0.0, 12.0, 64.0);
        fill_polys(c, &l_shape, FillRule::NonZero);
    });
    assert_eq!(alpha(&scissored, 8, 30), 1.0, "inside the scissor");
    assert_eq!(alpha(&scissored, 16, 30), 0.0, "outside the scissor");

    // A composite operation touches the pixels the shape covers, as with
    // the stencil path: destination-in keeps the red inside the L, erases
    // nothing the shape does not reach, and weights the edge by coverage.
    let kept = render(&device, &queue, |c| {
        let mut rect = Path::new();
        rect.rect(0.0, 0.0, 64.0, 64.0);
        c.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
        c.global_composite_operation(CompositeOperation::DestinationIn);
        let slanted: Vec<Poly> = vec![vec![
            (4.0, 4.0),
            (4.0, 60.0),
            (60.5, 60.0),
            (60.5, 44.0),
            (20.5, 44.0),
            (20.5, 4.0),
        ]];
        fill_polys(c, &slanted, FillRule::NonZero);
    });
    assert_eq!(alpha(&kept, 10, 10), 1.0, "inside the L: kept");
    assert_eq!(alpha(&kept, 40, 20), 1.0, "in the region, outside the L: untouched");
    assert!(
        (alpha(&kept, 20, 10) - 0.5).abs() <= 2.0 / 255.0,
        "the edge pixel: kept by its coverage"
    );

    let layered = render(&device, &queue, |c| {
        assert!(c.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        fill_polys(c, &l_shape, FillRule::NonZero);
        c.end_layer();
    });
    assert!(
        (alpha(&layered, 10, 10) - 0.5).abs() <= 1.0 / 255.0,
        "through the layer's opacity"
    );
}

/// Without antialiasing the fill takes the stencil path: every pixel is
/// in or out.
#[test]
fn aliased_fills_keep_hard_edges() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let px = render(&device, &queue, |c| {
        let mut paint = Paint::color(Color::black());
        paint.set_anti_alias(false);
        c.fill_path(&path_of(&shapes()[2].1), &paint);
    });
    assert!(px.chunks_exact(4).all(|c| c[3] == 0 || c[3] == 255));
}
