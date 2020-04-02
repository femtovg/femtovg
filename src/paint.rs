
use crate::geometry::Transform2D;
use crate::{
    Color,
    ImageId,
    LineCap,
    LineJoin,
    Baseline,
    FillRule,
    Align,
    Weight,
    WidthClass,
    FontStyle
};

#[derive(Copy, Clone, Debug)]
pub(crate) enum PaintFlavor {
    Color(Color),
    Image {
        id: ImageId,
        cx: f32,
        cy: f32,
        width: f32,
        height: f32,
        angle: f32,
        alpha: f32
    },
    LinearGradient {
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        start_color: Color,
        end_color: Color
    },
    BoxGradient {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        feather: f32,
        inner_color: Color,
        outer_color: Color
    },
    RadialGradient {
        cx: f32,
        cy: f32,
        in_radius: f32,
        out_radius: f32,
        inner_color: Color,
        outer_color: Color
    }
}

/// Struct controlling how graphical shapes are rendered.
///
/// The Paint struct is a relatively lightweight object which contains all the information needed to
/// display something on a canvas. Unlike the HTML canvas where the current drawing style is stored
/// in an internal stack this paint struct is simply passed to the relevant drawing methods on the canvas.
///
/// Clients code can have as many paints as they desire for different use cases and styles. This makes
/// the internal stack in the [Canvas](struct.Canvas.html) struct much lighter since it only needs to
/// contain the transform stack and current scissor rectangle.
///
/// # Example
/// ```
/// use gpucanvas::{Paint, Path, Color, Canvas, renderer::Void};
///
/// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
///
/// let fill_paint = Paint::color(Color::hex("454545"));
/// let mut stroke_paint = Paint::color(Color::hex("bababa"));
/// stroke_paint.set_stroke_width(4.0);
///
/// let mut path = Path::new();
/// path.rounded_rect(10.0, 10.0, 100.0, 100.0, 20.0);
/// canvas.fill_path(&mut path, fill_paint);
/// canvas.stroke_path(&mut path, stroke_paint);
/// ```
#[derive(Copy, Clone, Debug)]
pub struct Paint<'a> {
    pub(crate) flavor: PaintFlavor,
    pub(crate) transform: Transform2D,
    pub(crate) alpha_mask: Option<ImageId>,
    pub(crate) stroke_width: f32,
    pub(crate) shape_anti_alias: bool,
    pub(crate) stencil_strokes: bool,
    pub(crate) miter_limit: f32,
    pub(crate) line_cap_start: LineCap,
    pub(crate) line_cap_end: LineCap,
    pub(crate) line_join: LineJoin,
    pub(crate) font_family: &'a str,
    pub(crate) font_size: u32,
    pub(crate) font_blur: u8,
    pub(crate) font_weight: Weight,
    pub(crate) font_width_class: WidthClass,
    pub(crate) font_style: FontStyle,
    pub(crate) letter_spacing: f32,
    pub(crate) text_baseline: Baseline,
    pub(crate) text_align: Align,
    pub(crate) fill_rule: FillRule
}

impl Default for Paint<'_> {
    fn default() -> Self {
        Self {
            flavor: PaintFlavor::Color(Color::white()),
            transform: Default::default(),
            alpha_mask: Default::default(),
            shape_anti_alias: true,
            stencil_strokes: true,
            stroke_width: 1.0,
            miter_limit: 10.0,
            line_cap_start: Default::default(),
            line_cap_end: Default::default(),
            line_join: Default::default(),
            font_family: "NotoSans",
            font_size: 16,
            font_blur: 0,
            font_weight: Weight::Normal,
            font_width_class: WidthClass::Normal,
            font_style: FontStyle::Normal,
            letter_spacing: 0.0,
            text_baseline: Default::default(),
            text_align: Default::default(),
            fill_rule: Default::default(),
        }
    }
}

impl<'a> Paint<'a> {
    /// Creates a new solid color paint
    pub fn color(color: Color) -> Self {
        let mut new = Self::default();
        new.flavor = PaintFlavor::Color(color);
        new
    }

    /// Creates a new image pattern paint.
    ///
    /// * `id` - is handle to the image to render
    /// * `cx` `cy` - Specify the top-left location of the image pattern
    /// * `width` `height` - The size of one image
    /// * `angle` - Rotation around the top-left corner
    /// * `alpha` - Transparency applied on the image
    ///
    /// # Example
    /// ```
    /// use gpucanvas::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let image_id = canvas.create_image_file("examples/assets/rust-logo.png", ImageFlags::GENERATE_MIPMAPS).expect("Cannot create image");
    /// let fill_paint = Paint::image(image_id, 10.0, 10.0, 85.0, 85.0, 0.0, 1.0);
    ///
    /// let mut path = Path::new();
    /// path.rect(10.0, 10.0, 85.0, 85.0);
    /// canvas.fill_path(&mut path, fill_paint);
    /// ```
    pub fn image(id: ImageId, cx: f32, cy: f32, width: f32, height: f32, angle: f32, alpha: f32) -> Self {
        let mut new = Self::default();
        new.flavor = PaintFlavor::Image { id, cx, cy, width, height, angle, alpha };
        new
    }

    /// Creates and returns a linear gradient paint.
    ///
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
    pub fn linear_gradient(start_x: f32, start_y: f32, end_x: f32, end_y: f32, start_color: Color, end_color: Color) -> Self {
        let mut new = Self::default();

        new.flavor = PaintFlavor::LinearGradient {
            start_x, start_y, end_x, end_y, start_color, end_color
        };

        new
    }

    /// Creates and returns a box gradient.
    ///
    /// Box gradient is a feathered rounded rectangle, it is useful for rendering
    /// drop shadows or highlights for boxes. Parameters (x,y) define the top-left corner of the rectangle,
    /// (w,h) define the size of the rectangle, r defines the corner radius, and f feather. Feather defines how blurry
    /// the border of the rectangle is. Parameter inner_color specifies the inner color and outer_color the outer color of the gradient.
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
    pub fn box_gradient(x: f32, y: f32, width: f32, height: f32, radius: f32, feather: f32, inner_color: Color, outer_color: Color) -> Self {
        let mut new = Self::default();

        new.flavor = PaintFlavor::BoxGradient {
            x, y, width, height, radius, feather, inner_color, outer_color
        };

        new
    }

    /// Creates and returns a radial gradient.
    ///
    /// Parameters (cx,cy) specify the center, inr and outr specify
    /// the inner and outer radius of the gradient, icol specifies the start color and ocol the end color.
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
    pub fn radial_gradient(cx: f32, cy: f32, in_radius: f32, out_radius: f32, inner_color: Color, outer_color: Color) -> Self {
        let mut new = Self::default();

        new.flavor = PaintFlavor::RadialGradient {
            cx, cy, in_radius, out_radius, inner_color, outer_color
        };

        new
    }

    /// Creates a new solid color paint
    pub fn set_color(&mut self, color: Color) {
        self.flavor = PaintFlavor::Color(color);
    }

    pub fn alpha_mask(&self) -> Option<ImageId> {
        self.alpha_mask
    }

    pub fn set_alpha_mask(&mut self, image_id: Option<ImageId>) {
        self.alpha_mask = image_id;
    }

    /// Returns boolean if the shapes drawn with this paint will be antialiased.
    pub fn anti_alias(&self) -> bool {
        self.shape_anti_alias
    }

    /// Sets whether shapes drawn with this paint will be anti aliased. Enabled by default.
    pub fn set_anti_alias(&mut self, value: bool) {
        self.shape_anti_alias = value;
    }

    /// True if this paint uses higher quality stencil strokes.
    pub fn stencil_strokes(&self) -> bool {
        self.stencil_strokes
    }

    /// Sets whether to use higher quality stencil strokes.
    pub fn set_stencil_strokes(&mut self, value: bool) {
        self.stencil_strokes = value;
    }

    /// Returns the current stroke line width.
    pub fn stroke_width(&self) -> f32 {
        self.stroke_width
    }

    /// Sets the stroke width for shapes stroked with this paint.
    pub fn set_stroke_width(&mut self, width: f32) {
        self.stroke_width = width;
    }

    /// Getter for the miter limit
    pub fn miter_limit(&self) -> f32 {
        self.miter_limit
    }

    /// Sets the limit at which a sharp corner is drawn beveled.
    ///
    /// If the miter at a corner exceeds this limit, LineJoin is replaced with LineJoin::Bevel.
    pub fn set_miter_limit(&mut self, limit: f32) {
        self.miter_limit = limit;
    }

    /// Returns the current start line cap for this paint.
    pub fn line_cap_start(&self) -> LineCap {
        self.line_cap_start
    }

    /// Returns the current start line cap for this paint.
    pub fn line_cap_end(&self) -> LineCap {
        self.line_cap_end
    }

    /// Sets how the start and end of the line (cap) is drawn
    ///
    /// By default it's set to LineCap::Butt
    pub fn set_line_cap(&mut self, cap: LineCap) {
        self.line_cap_start = cap;
        self.line_cap_end = cap;
    }

    /// Sets how the beggining cap of the line is drawn
    ///
    /// By default it's set to LineCap::Butt
    pub fn set_line_cap_start(&mut self, cap: LineCap) {
        self.line_cap_start = cap;
    }

    /// Sets how the end cap of the line is drawn
    ///
    /// By default it's set to LineCap::Butt
    pub fn set_line_cap_end(&mut self, cap: LineCap) {
        self.line_cap_end = cap;
    }

    /// Returns the current line join for this paint.
    pub fn line_join(&self) -> LineJoin {
        self.line_join
    }

    /// Sets how sharp path corners are drawn.
    ///
    /// By default it's set to LineJoin::Miter
    pub fn set_line_join(&mut self, join: LineJoin) {
        self.line_join = join;
    }

    /// Returns the font name that is used when drawing text with this paint
    pub fn font_family(&self) -> &str {
        &self.font_family
    }

    /// Sets the font name for text drawn with this paint
    ///
    /// This needs to be the font family name. Eg. "NotoSans" not the postscript name (NotoSans-Regular)
    /// Only has effect on canvas text operations
    pub fn set_font_family(&mut self, name: &'a str) {
        self.font_family = name;
    }

    /// Returns the current font size
    ///
    /// Only has effect on canvas text operations
    pub fn font_size(&self) -> u32 {
        self.font_size
    }

    /// Sets the font size.
    ///
    /// Only has effect on canvas text operations
    pub fn set_font_size(&mut self, size: u32) {
        self.font_size = size;
    }

    /// Returns the current letter spacing
    pub fn letter_spacing(&self) -> f32 {
        self.letter_spacing
    }

    /// Sets the letter spacing for this paint
    ///
    /// Only has effect on canvas text operations
    pub fn set_letter_spacing(&mut self, spacing: f32) {
        self.letter_spacing = spacing;
    }

    /// Returns the current font blur
    pub fn font_blur(&self) -> u8 {
        self.font_blur
    }

    /// Sets the font blur radius
    ///
    /// Useful for implementing text shadow. Only has effect on canvas text operations
    pub fn set_font_blur(&mut self, blur: u8) {
        self.font_blur = blur;
    }

    /// Returns the current vertical align
    pub fn text_baseline(&self) -> Baseline {
        self.text_baseline
    }

    /// Sets the text vertical alignment for this paint
    ///
    /// Only has effect on canvas text operations
    pub fn set_text_baseline(&mut self, align: Baseline) {
        self.text_baseline = align;
    }

    /// Returns the current horizontal align
    pub fn text_align(&self) -> Align {
        self.text_align
    }

    /// Sets the text horizontal alignment for this paint
    ///
    /// Only has effect on canvas text operations
    pub fn set_text_align(&mut self, align: Align) {
        self.text_align = align;
    }

    pub fn font_weight(&self) -> Weight {
        self.font_weight
    }

    pub fn set_font_weight(&mut self, weight: Weight) {
        self.font_weight = weight;
    }

    pub fn font_width_class(&self) -> WidthClass {
        self.font_width_class
    }

    pub fn set_font_width_class(&mut self, class: WidthClass) {
        self.font_width_class = class;
    }

    pub fn font_style(&self) -> FontStyle {
        self.font_style
    }

    pub fn set_font_style(&mut self, style: FontStyle) {
        self.font_style = style;
    }

    /// Retrieves the current fill rule setting for this paint
    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    /// Sets the current rule to be used when filling a path
    ///
    /// https://developer.mozilla.org/en-US/docs/Web/SVG/Attribute/fill-rule
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        self.fill_rule = rule;
    }

    pub(crate) fn mul_alpha(&mut self, a: f32) {
        match &mut self.flavor {
            PaintFlavor::Color(color) => {
                color.a *= a;
            }
            PaintFlavor::Image { alpha, ..} => {
                *alpha *= a;
            }
            PaintFlavor::LinearGradient { start_color, end_color, ..} => {
                start_color.a *= a;
                end_color.a *= a;
            }
            PaintFlavor::BoxGradient { inner_color, outer_color, ..} => {
                inner_color.a *= a;
                outer_color.a *= a;
            }
            PaintFlavor::RadialGradient { inner_color, outer_color, ..} => {
                inner_color.a *= a;
                outer_color.a *= a;
            }
        }
    }
}
