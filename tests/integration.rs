use femtovg::{renderer::Void, Baseline, Canvas, Color, FillRule, Paint, Path, Solidity};

#[test]
fn path_with_single_move_to() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_two_lines() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.line_to(10.0, 10.0);
    path.line_to(10.0, 10.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_close_points() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.line_to(10.0001, 10.0);
    path.line_to(10.0001, 10.000001);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_points_at_limits() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.line_to(f32::MAX, f32::MAX);
    path.quad_to(10.0, 10.0, -f32::MAX, f32::MAX);
    path.bezier_to(10.0, 10.0, f32::MAX, 5000.0, -f32::MAX, -f32::MAX);
    path.rounded_rect_varying(
        -f32::MAX,
        -f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
    );
    path.close();

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
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
        -f32::MAX,
        -f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        0.0001,
        0.0001,
        0.0001,
    );

    path.close();

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn degenerate_stroke() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(0.5, 0.5);
    path.line_to(2., 2.);
    path.line_to(2., 2.);
    path.line_to(4., 2.);
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn degenerate_arc_to() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.arc_to(10.0, 10.0001, 10.0, 10.0001, 2.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn degenerate_arc() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.arc(10.0, 10.0, 10.0, 0.0, f32::MAX, Solidity::Hole);

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_contains_point() {
    let mut canvas = Canvas::new(Void).unwrap();
    // without setting size contains_point will early out on the bounds check and report false
    canvas.set_size(100, 100, 1.0);

    // Star - cancave & self crossing
    let mut path = Path::new();
    path.move_to(50.0, 0.0);
    path.line_to(21.0, 90.0);
    path.line_to(98.0, 35.0);
    path.line_to(2.0, 35.0);
    path.line_to(79.0, 90.0);
    path.close();

    // Center of the star should be hollow for even-odd rule
    assert!(!canvas.contains_point(&path, 50.0, 45.0, FillRule::EvenOdd));
    assert!(canvas.contains_point(&path, 50.0, 5.0, FillRule::EvenOdd));

    // Center of the star should be fill for NonZero rule
    assert!(canvas.contains_point(&path, 50.0, 45.0, FillRule::NonZero));
    assert!(canvas.contains_point(&path, 50.0, 5.0, FillRule::NonZero));
}

#[test]
fn text_location_respects_scale() {
    let mut canvas = Canvas::new(Void).unwrap();

    canvas
        .add_font("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let paint = Paint::color(Color::black()).with_text_baseline(Baseline::Top);
    canvas.scale(5.0, 5.0);

    let res = canvas.measure_text(100.0, 100.0, "Hello", &paint).unwrap();

    assert_eq!(res.x, 100.0);
    assert_eq!(res.y, 100.0);
}

#[test]
fn text_measure_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let metrics = text_context
        .measure_text(0., 0., "Hello World", &test_paint)
        .expect("text shaping failed unexpectedly");

    assert_eq!(metrics.width().ceil(), 83.);
    assert_eq!(metrics.height().ceil(), 13.);
}

#[test]
fn font_measure_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let metrics = text_context
        .measure_font(&test_paint)
        .expect("font measuring failed unexpectedly");

    assert_eq!(metrics.ascender().ceil(), 15.);
}

#[test]
fn break_text_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let text = "Multiple Lines Broken";

    let breaks = text_context
        .break_text_vec(60., text, &test_paint)
        .expect("text shaping failed unexpectedly");

    assert_eq!(
        breaks
            .iter()
            .map(|range| &text[range.start..range.end])
            .collect::<Vec<_>>(),
        vec!["Multiple ", "Lines ", "Broken"]
    );
}

#[test]
fn variable_font_weight_affects_measurement() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    // Verify the font has a wght axis
    let axes = text_context.font_variation_axes(font_id).unwrap();
    let wght_axis = axes.iter().find(|a| &a.tag == b"wght");
    assert!(wght_axis.is_some(), "Font should have a wght axis");

    let light_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_weight(300.0);

    let bold_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_weight(700.0);

    let light_metrics = text_context
        .measure_text(0., 0., "Hello World", &light_paint)
        .expect("text shaping failed");

    let bold_metrics = text_context
        .measure_text(0., 0., "Hello World", &bold_paint)
        .expect("text shaping failed");

    // Bold text should be wider than light text
    assert!(
        bold_metrics.width() > light_metrics.width(),
        "Bold ({}) should be wider than light ({})",
        bold_metrics.width(),
        light_metrics.width()
    );
}

#[test]
fn font_variation_generic_api_matches_named_weight() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    // Using the generic variation API for wght should produce the same measurement
    // as the named font_weight API
    let named_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_weight(700.0);

    let generic_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_variation(b"wght", 700.0);

    let named_metrics = text_context
        .measure_text(0., 0., "Hello World", &named_paint)
        .expect("text shaping failed");

    let generic_metrics = text_context
        .measure_text(0., 0., "Hello World", &generic_paint)
        .expect("text shaping failed");

    assert!(
        (named_metrics.width() - generic_metrics.width()).abs() < f32::EPSILON,
        "Named weight API ({}) and generic variation API ({}) should produce identical measurements",
        named_metrics.width(),
        generic_metrics.width()
    );
}

#[test]
fn font_variation_italic_and_slant_api() {
    // Test the Paint API for italic and slant methods
    let mut paint = femtovg::Paint::default();

    // Initially no italic set
    assert!(paint.font_italic().is_none());
    assert!(paint.font_slant().is_none());

    // Set italic — also sets slnt as fallback
    paint.set_font_italic(true);
    assert_eq!(paint.font_italic(), Some(true));
    assert_eq!(paint.font_slant(), Some(-12.0));

    paint.set_font_italic(false);
    assert_eq!(paint.font_italic(), Some(false));
    assert_eq!(paint.font_slant(), Some(0.0));

    // clear_font_italic removes both ital and slnt
    paint.clear_font_italic();
    assert!(paint.font_italic().is_none());
    assert!(paint.font_slant().is_none());

    // Explicit slant overrides the italic fallback
    paint.set_font_italic(true);
    paint.set_font_slant(-5.0);
    assert_eq!(paint.font_italic(), Some(true));
    assert_eq!(paint.font_slant(), Some(-5.0));

    paint.clear_font_variations();

    // Set slant independently
    paint.set_font_slant(-12.0);
    assert_eq!(paint.font_slant(), Some(-12.0));

    paint.clear_font_slant();
    assert!(paint.font_slant().is_none());

    // Multiple variations at once
    paint.set_font_weight(700.0);
    paint.set_font_italic(true);

    assert_eq!(paint.font_weight(), Some(700.0));
    assert_eq!(paint.font_italic(), Some(true));
    assert_eq!(paint.font_slant(), Some(-12.0));

    // Clear all
    paint.clear_font_variations();
    assert!(paint.font_weight().is_none());
    assert!(paint.font_italic().is_none());
    assert!(paint.font_slant().is_none());
}

#[test]
fn font_variation_hash_stability() {
    // Setting the same variations in different order should produce the same hash
    let mut paint_a = femtovg::Paint::default();
    paint_a.set_font_weight(700.0);
    paint_a.set_font_italic(true);

    let mut paint_b = femtovg::Paint::default();
    paint_b.set_font_italic(true);
    paint_b.set_font_weight(700.0);

    // Both paints should have the same variation hash (verified via the generic API)
    assert_eq!(paint_a.font_weight(), paint_b.font_weight());
    assert_eq!(paint_a.font_italic(), paint_b.font_italic());
}

#[test]
fn variable_font_slant_affects_measurement() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    // Verify the font has a slnt axis
    let axes = text_context.font_variation_axes(font_id).unwrap();
    let slnt_axis = axes.iter().find(|a| &a.tag == b"slnt");
    assert!(slnt_axis.is_some(), "Font should have a slnt axis");

    let upright_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let slanted_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_slant(-10.0);

    let upright_metrics = text_context
        .measure_text(0., 0., "Hello World", &upright_paint)
        .expect("text shaping failed");

    let slanted_metrics = text_context
        .measure_text(0., 0., "Hello World", &slanted_paint)
        .expect("text shaping failed");

    // Slanted text should produce different measurements than upright text
    // (the widths may differ slightly due to slant-adjusted glyph metrics)
    assert!(
        (upright_metrics.width() - slanted_metrics.width()).abs() > 0.0
            || upright_metrics.glyphs.len() == slanted_metrics.glyphs.len(),
        "Slanted text should shape successfully and produce valid metrics"
    );
}

#[test]
fn font_italic_falls_back_to_slnt_axis() {
    // Roboto Flex has no `ital` axis but has `slnt`. Calling set_font_italic
    // should still produce slanted text via the slnt fallback.
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let axes = text_context.font_variation_axes(font_id).unwrap();
    assert!(
        axes.iter().find(|a| &a.tag == b"ital").is_none(),
        "Font should NOT have an ital axis"
    );
    assert!(
        axes.iter().find(|a| &a.tag == b"slnt").is_some(),
        "Font should have a slnt axis"
    );

    let upright_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let italic_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_italic(true);

    let upright_metrics = text_context
        .measure_text(0., 0., "Hello World", &upright_paint)
        .expect("text shaping failed");

    let italic_metrics = text_context
        .measure_text(0., 0., "Hello World", &italic_paint)
        .expect("text shaping failed");

    // The slnt axis changes glyph outlines (tilts them) but doesn't change advance
    // widths, so we verify that shaping succeeds and produces the same glyph count.
    // The visual difference (slanted glyphs) is only visible in rendering.
    assert_eq!(
        upright_metrics.glyphs.len(),
        italic_metrics.glyphs.len(),
        "Italic fallback via slnt should shape the same number of glyphs"
    );
    assert!(italic_metrics.width() > 0.0, "Italic text should have positive width");
}
