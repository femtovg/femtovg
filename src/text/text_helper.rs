
use crate::{
    Canvas,
    Renderer,
    Path,
    Paint,
    ErrorKind,
    FillRule,
    geometry::Transform2D
};

use super::{
    TextLayout,
    TextStyle,
    RenderStyle
};

pub fn render_text<T: Renderer>(canvas: &mut Canvas<T>, text_layout: &TextLayout, style: &TextStyle<'_>, paint: &Paint, invscale: f32) -> Result<(), ErrorKind> {

    let mut paint = *paint;
    paint.set_fill_rule(FillRule::EvenOdd);
    paint.set_anti_alias(false);

    for glyph in &text_layout.glyphs {
        let mut path = {
            let font = canvas.fontdb.get_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
            let font = ttf_parser::Font::from_data(&font.data, 0).ok_or(ErrorKind::FontParseError)?;

            let units_per_em = font.units_per_em().ok_or(ErrorKind::FontParseError)?;
            let scale = paint.font_size() as f32 / units_per_em as f32;

            let mut transform = Transform2D::identity();
            transform.scale(scale, -scale);
            transform.translate(glyph.x * invscale, glyph.y * invscale);

            let mut path_builder = TransformedPathBuilder(Path::new(), transform);
            font.outline_glyph(ttf_parser::GlyphId(glyph.codepoint as u16), &mut path_builder);

            path_builder.0
        };

        if let RenderStyle::Stroke { width } = style.render_style {
            paint.set_stroke_width(width as f32);
            canvas.stroke_path(&mut path, paint);
        } else {
            canvas.fill_path(&mut path, paint);
        }
    }

    Ok(())
}

struct TransformedPathBuilder(Path, Transform2D);

impl ttf_parser::OutlineBuilder for TransformedPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        self.0.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        self.0.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        let (x1, y1) = self.1.transform_point(x1, y1);
        self.0.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        let (x1, y1) = self.1.transform_point(x1, y1);
        let (x2, y2) = self.1.transform_point(x2, y2);
        self.0.bezier_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.0.close();
    }
}