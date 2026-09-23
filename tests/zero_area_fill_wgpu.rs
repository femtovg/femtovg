//! Headless GPU tests: a fill of a contour that encloses nothing draws
//! nothing (femtovg/femtovg#341). A bare `<line>` or an open path under
//! SVG's default black fill used to ink a two-pixel line: the antialiasing
//! fringe was extruded on both sides of a contour with no interior. A thin
//! sliver that does enclose area still draws. Skips without a GPU adapter.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

mod common;
use common::headless_device;

const W: u32 = 128;
const H: u32 = 128;

fn fill(device: &wgpu::Device, queue: &wgpu::Queue, build: impl FnOnce(&mut Path)) -> f64 {
    let px = common::render_rgba(
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
            canvas.fill_path(&path, &paint);
        },
    );
    px.chunks_exact(4).map(|c| c[3] as f64 / 255.0).sum()
}

#[test]
fn a_contour_that_encloses_nothing_fills_nothing() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let cases: [(&str, Box<dyn Fn(&mut Path)>); 4] = [
        (
            "a bare line, closed",
            Box::new(|p| {
                p.move_to(10.0, 10.0);
                p.line_to(10.0, 90.0);
                p.close();
            }),
        ),
        (
            "a bare line, open",
            Box::new(|p| {
                p.move_to(25.0, 10.0);
                p.line_to(25.0, 90.0);
            }),
        ),
        (
            "three collinear points",
            Box::new(|p| {
                p.move_to(40.0, 10.0);
                p.line_to(40.0, 50.0);
                p.line_to(40.0, 90.0);
                p.close();
            }),
        ),
        (
            "a diagonal with fractional coordinates",
            Box::new(|p| {
                p.move_to(12.345, 17.891);
                p.line_to(67.891, 95.123);
                p.line_to(40.118, 56.507);
                p.close();
            }),
        ),
    ];
    for (name, build) in cases {
        let ink = fill(&device, &queue, build);
        assert!(ink < 0.05, "{name}: {ink:.2} px of ink, expected none");
    }
}

/// A sliver a third of a pixel wide encloses area and keeps drawing; a
/// shape wider than the fringe is unaffected.
#[test]
fn shapes_that_enclose_area_still_fill() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let sliver = fill(&device, &queue, |p| {
        p.move_to(20.0, 20.0);
        p.line_to(100.0, 60.0);
        p.line_to(100.3, 60.0);
        p.line_to(20.3, 20.0);
        p.close();
    });
    assert!(sliver > 5.0, "the sliver draws: {sliver:.2} px of ink");
    let square = fill(&device, &queue, |p| p.rect(20.0, 20.0, 40.0, 40.0));
    assert!((square - 1600.0).abs() < 1.0, "{square:.2} px, expected 1600");
}
