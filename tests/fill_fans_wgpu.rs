//! A fill reaches the GPU as a fan drawn through indices the renderer builds
//! each frame: fills longer than any the renderer drew before, a concave one
//! through the stencil, and a contour longer than one fan all cover their
//! area.
#![cfg(feature = "wgpu")]

mod common;

use femtovg::{Color, Paint, Path};

const SIZE: u32 = 1024;

/// The closed outline through `corners` about the canvas centre, at
/// alternating radii, with each side cut into `per_side` pieces, and the area
/// it encloses.
fn outline(corners: usize, radii: [f64; 2], per_side: usize) -> (Path, f64) {
    let corner = |i: usize| {
        let a = i as f64 / corners as f64 * std::f64::consts::TAU;
        let r = radii[i % 2];
        [512.0 + r * a.cos(), 512.0 + r * a.sin()]
    };
    let points: Vec<[f32; 2]> = (0..corners)
        .flat_map(|i| {
            let ([x0, y0], [x1, y1]) = (corner(i), corner((i + 1) % corners));
            (0..per_side).map(move |k| {
                let t = k as f64 / per_side as f64;
                [(x0 + (x1 - x0) * t) as f32, (y0 + (y1 - y0) * t) as f32]
            })
        })
        .collect();
    let mut path = Path::new();
    path.move_to(points[0][0], points[0][1]);
    for &[x, y] in &points[1..] {
        path.line_to(x, y);
    }
    path.close();
    let twice_area: f64 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64)
        .sum();
    (path, twice_area.abs() / 2.0)
}

#[test]
fn fills_longer_than_any_before_cover_their_area() {
    let Some((device, queue)) = common::headless_device() else {
        return;
    };
    // In one renderer, each longer than the last: a 600-gon, a star of 3,000
    // points, a circle of 196,000 points - three fans of about a third each,
    // its points further apart than two that count as one - and a short one
    // again.
    let shapes = [
        (600, [450.0, 450.0], 1),
        (10, [450.0, 180.0], 300),
        (196_000, [450.0, 450.0], 1),
        (600, [450.0, 450.0], 1),
    ];
    let frames = common::render_frames(
        &device,
        &queue,
        SIZE,
        SIZE,
        Color::black(),
        shapes.len(),
        |canvas, frame| {
            let (corners, radii, per_side) = shapes[frame];
            let (path, _) = outline(corners, radii, per_side);
            canvas.fill_path(&path, &Paint::color(Color::white()));
        },
    );

    for ((corners, radii, per_side), pixels) in shapes.into_iter().zip(&frames) {
        let (_, area) = outline(corners, radii, per_side);
        let covered: f64 = pixels.chunks(4).map(|pixel| pixel[0] as f64 / 255.0).sum();
        assert!(
            (covered - area).abs() < 0.002 * area,
            "{} points: {covered:.1} px covered of {area:.1}",
            corners * per_side
        );
    }
}
