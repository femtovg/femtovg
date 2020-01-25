
use gpucanvas::{
    Canvas,
    Path,
    Paint,
    Color,
    renderer::Void
};

#[test]
fn path_with_one_move_to_does_not_panic() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_two_lines_to_does_not_panic() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.line_to(10.0, 10.0);
    path.line_to(10.0, 10.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_close_points() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.line_to(10.0001, 10.0);
    path.line_to(10.0001, 10.000001);
    canvas.fill_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_points_at_limits() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.line_to(std::f32::MAX, std::f32::MAX);
    path.quad_to(10.0, 10.0, -std::f32::MAX, std::f32::MAX);
    path.bezier_to(10.0, 10.0, std::f32::MAX, 5000.0, -std::f32::MAX, -std::f32::MAX);
    path.rounded_rect_varying(
        -std::f32::MAX, -std::f32::MAX,
        std::f32::MAX, std::f32::MAX,
        std::f32::MAX, std::f32::MAX,
        std::f32::MAX, std::f32::MAX
    );
    path.close();

    canvas.fill_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_points_around_zero() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(0.0, 0.0);
    path.line_to(0.0, 0.0);
    path.line_to(0.0001, 0.0000003);
    path.quad_to(0.002, 0.0001, -0.002, 0.0001);
    path.bezier_to(0.0001, 0.002, -0.002, 0.0001, -0.002, 0.0001);
    path.rounded_rect_varying(
        -std::f32::MAX, -std::f32::MAX,
        std::f32::MAX, std::f32::MAX,
        std::f32::MAX, 0.0001,
        0.0001, 0.0001
    );

    path.close();

    canvas.fill_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&mut path, Paint::color(Color::rgb(100, 100, 100)));
}
