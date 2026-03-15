use femtovg::{Baseline, Canvas, Color, FillRule, Paint, Path, Solidity, StrokeSettings, TextSettings, renderer::Void};

#[test]
fn path_with_single_move_to() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to([10.0, 10.0]);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)), FillRule::default());
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn path_with_two_lines() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.line_to([10.0, 10.0]);
    path.line_to([10.0, 10.0]);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)), FillRule::default());
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn path_with_close_points() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to([10.0, 10.0]);
    path.line_to([10.0001, 10.0]);
    path.line_to([10.0001, 10.000001]);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)), FillRule::default());
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn path_with_points_at_limits() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to([10.0, 10.0]);
    path.line_to([f32::MAX, f32::MAX]);
    path.quad_to([10.0, 10.0], [-f32::MAX, f32::MAX]);
    path.bezier_to([10.0, 10.0], [f32::MAX, 5000.0], [-f32::MAX, -f32::MAX]);
    path.rounded_rect_varying(
        [-f32::MAX, -f32::MAX],
        [f32::MAX, f32::MAX],
        f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
    );
    path.close();

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)), FillRule::default());
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn path_with_points_around_zero() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to([0.0, 0.0]);
    path.line_to([0.0, 0.0]);
    path.line_to([0.0001, 0.0000003]);
    path.quad_to([0.002, 0.0001], [-0.002, 0.0001]);
    path.bezier_to([0.0001, 0.002], [-0.002, 0.0001], [-0.002, 0.0001]);
    path.rounded_rect_varying(
        [-f32::MAX, -f32::MAX],
        [f32::MAX, f32::MAX],
        f32::MAX,
        0.0001,
        0.0001,
        0.0001,
    );

    path.close();

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)), FillRule::default());
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn degenerate_stroke() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to([0.5, 0.5]);
    path.line_to([2., 2.]);
    path.line_to([2., 2.]);
    path.line_to([4., 2.]);
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn degenerate_arc_to() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to([10.0, 10.0]);
    path.arc_to([10.0, 10.0001], [10.0, 10.0001], 2.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)), FillRule::default());
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn degenerate_arc() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to([10.0, 10.0]);
    path.arc([10.0, 10.0], 10.0, 0.0, f32::MAX, Solidity::Hole);

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)), FillRule::default());
    canvas.stroke_path(
        &path,
        &Paint::color(Color::rgb(100, 100, 100)),
        &StrokeSettings::default(),
    );
}

#[test]
fn path_contains_point() {
    let mut canvas = Canvas::new(Void).unwrap();
    canvas.set_size(100, 100, 1.0);

    let mut path = Path::new();
    path.move_to([50.0, 0.0]);
    path.line_to([21.0, 90.0]);
    path.line_to([98.0, 35.0]);
    path.line_to([2.0, 35.0]);
    path.line_to([79.0, 90.0]);
    path.close();

    assert!(!canvas.contains_point(&path, [50.0, 45.0], FillRule::EvenOdd));
    assert!(canvas.contains_point(&path, [50.0, 5.0], FillRule::EvenOdd));

    assert!(canvas.contains_point(&path, [50.0, 45.0], FillRule::NonZero));
    assert!(canvas.contains_point(&path, [50.0, 5.0], FillRule::NonZero));
}

#[test]
fn text_location_respects_scale() {
    let mut canvas = Canvas::new(Void).unwrap();

    let font_id = canvas
        .add_font("examples/assets/Roboto-Regular.ttf")
        .expect("Font not found");

    let text_settings = TextSettings::new(&[font_id], 16.0).with_baseline(Baseline::Top);
    canvas.scale([5.0, 5.0]);

    let res = canvas.measure_text(100.0, 100.0, "Hello", &text_settings).unwrap();

    assert_eq!(res.x, 100.0);
    assert_eq!(res.y, 100.0);
}

#[test]
fn text_measure_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/Roboto-Regular.ttf")
        .expect("Font not found");

    let text_settings = TextSettings::new(&[font_id], 16.);

    let metrics = text_context
        .measure_text(0., 0., "Hello World", &text_settings)
        .expect("text shaping failed unexpectedly");

    assert_eq!(metrics.width().ceil(), 83.);
    assert_eq!(metrics.height().ceil(), 13.);
}

#[test]
fn font_measure_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/Roboto-Regular.ttf")
        .expect("Font not found");

    let text_settings = TextSettings::new(&[font_id], 16.);

    let metrics = text_context
        .measure_font(&text_settings)
        .expect("font measuring failed unexpectedly");

    assert_eq!(metrics.ascender().ceil(), 17.);
}

#[test]
fn break_text_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/Roboto-Regular.ttf")
        .expect("Font not found");

    let text_settings = TextSettings::new(&[font_id], 16.);

    let text = "Multiple Lines Broken";

    let breaks = text_context
        .break_text_vec(60., text, &text_settings)
        .expect("text shaping failed unexpectedly");

    assert_eq!(
        breaks
            .iter()
            .map(|range| &text[range.start..range.end])
            .collect::<Vec<_>>(),
        vec!["Multiple ", "Lines ", "Broken"]
    );
}
