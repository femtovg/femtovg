// TODO: prefix paint creation functions with make_ or new_
// so that they are easier to find when autocompleting

use crate::{geometry::Position, Align, Baseline, Color, FillRule, FontId, ImageId, LineCap, LineJoin};

#[derive(Copy, Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub(crate) struct GradientStop(pub f32, pub Color);

// We use MultiStopGradient as a key since we cache them. We either need
// to define Hash (for HashMap) or Ord for (BTreeMap).
impl Eq for GradientStop {}
impl Ord for GradientStop {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if (other.0, other.1) < (self.0, self.1) {
            std::cmp::Ordering::Less
        } else if (self.0, self.1) < (other.0, other.1) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }
}

impl PartialOrd for GradientStop {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub(crate) type MultiStopGradient = [GradientStop; 24];

#[allow(clippy::large_enum_variant)]
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub(crate) enum GradientColors {
    TwoStop {
        start_color: Color,
        end_color: Color,
    },
    MultiStop {
        // We support up to 16 stops.
        stops: MultiStopGradient,
    },
}
impl GradientColors {
    fn mul_alpha(&mut self, a: f32) {
        match self {
            GradientColors::TwoStop { start_color, end_color } => {
                start_color.a *= a;
                end_color.a *= a;
            }
            GradientColors::MultiStop { stops } => {
                for stop in stops {
                    stop.1.a *= a;
                }
            }
        }
    }
    fn from_stops(stops: &[(f32, Color)]) -> GradientColors {
        if stops.is_empty() {
            // No stops, we use black.
            GradientColors::TwoStop {
                start_color: Color::black(),
                end_color: Color::black(),
            }
        } else if stops.len() == 1 {
            // One stop devolves to a solid color fill (but using the gradient shader variation).
            GradientColors::TwoStop {
                start_color: stops[0].1,
                end_color: stops[0].1,
            }
        } else if stops.len() == 2 && stops[0].0 <= 0.0 && stops[1].0 >= 1.0 {
            // Two stops takes the classic gradient path, so long as the stop positions are at
            // the extents (if the stop positions are inset then we'll fill to them).
            GradientColors::TwoStop {
                start_color: stops[0].1,
                end_color: stops[1].1,
            }
        } else {
            // Actual multistop gradient. We copy out the stops and then use a stop with a
            // position > 1.0 as a sentinel. GradientStore ignores stop positions > 1.0
            // when synthesizing the gradient texture.
            let mut out_stops: [GradientStop; 24] = Default::default();
            for i in 0..24 {
                if i < stops.len() {
                    out_stops[i] = GradientStop(stops[i].0, stops[i].1);
                } else {
                    out_stops[i] = GradientStop(2.0, Color::black());
                }
            }
            GradientColors::MultiStop { stops: out_stops }
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub(crate) enum PaintFlavor {
    Color(Color),
    #[cfg_attr(feature = "serde", serde(skip))]
    Image {
        id: ImageId,
        center: Position,
        width: f32,
        height: f32,
        angle: f32,
        tint: Color,
    },
    LinearGradient {
        start: Position,
        end: Position,
        colors: GradientColors,
    },
    BoxGradient {
        pos: Position,
        width: f32,
        height: f32,
        radius: f32,
        feather: f32,
        colors: GradientColors,
    },
    RadialGradient {
        center: Position,
        in_radius: f32,
        out_radius: f32,
        colors: GradientColors,
    },
}

// Convenience method to fetch the GradientColors out of a PaintFlavor
impl PaintFlavor {
    pub(crate) fn mul_alpha(&mut self, a: f32) {
        match self {
            PaintFlavor::Color(color) => {
                color.a *= a;
            }
            PaintFlavor::Image { tint, .. } => {
                tint.a *= a;
            }
            PaintFlavor::LinearGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            PaintFlavor::BoxGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            PaintFlavor::RadialGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
        }
    }

    pub(crate) fn gradient_colors(&self) -> Option<&GradientColors> {
        match self {
            PaintFlavor::LinearGradient { colors, .. } => Some(colors),
            PaintFlavor::BoxGradient { colors, .. } => Some(colors),
            PaintFlavor::RadialGradient { colors, .. } => Some(colors),
            _ => None,
        }
    }

    /// Returns true if this paint is an untransformed image paint without anti-aliasing at the edges in case of a fill
    pub(crate) fn is_straight_tinted_image(&self, shape_anti_alias: bool) -> bool {
        matches!(self, &PaintFlavor::Image { angle, .. } if angle == 0.0 && !shape_anti_alias)
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum GlyphTexture {
    None,
    AlphaMask(ImageId),
    ColorTexture(ImageId),
}

impl Default for GlyphTexture {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StrokeSettings {
    pub(crate) stencil_strokes: bool,
    pub(crate) miter_limit: f32,
    pub(crate) line_width: f32,
    pub(crate) line_cap_start: LineCap,
    pub(crate) line_cap_end: LineCap,
    pub(crate) line_join: LineJoin,
}

impl Default for StrokeSettings {
    fn default() -> Self {
        Self {
            stencil_strokes: true,
            miter_limit: 10.0,
            line_width: 1.0,
            line_cap_start: Default::default(),
            line_cap_end: Default::default(),
            line_join: Default::default(),
        }
    }
}

/// Struct controlling how graphical shapes are rendered.
///
/// The Paint struct is a relatively lightweight object which contains all the information needed to
/// display something on the canvas. Unlike the HTML canvas where the current drawing style is stored
/// in an internal stack this paint struct is simply passed to the relevant drawing methods on the canvas.
///
/// Clients code can have as many paints as they desire for different use cases and styles. This makes
/// the internal stack in the [Canvas](struct.Canvas.html) struct much lighter since it only needs to
/// contain the transform stack and current scissor rectangle.
///
/// # Example
/// ```
/// use femtovg::{Paint, Path, Color, Canvas, renderer::Void};
///
/// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
///
/// let fill_paint = Paint::color(Color::hex("454545"));
/// let mut stroke_paint = Paint::color(Color::hex("bababa"));
/// stroke_paint.set_line_width(4.0);
///
/// let mut path = Path::new();
/// path.rounded_rect(10.0, 10.0, 100.0, 100.0, 20.0);
/// canvas.fill_path(&mut path, &fill_paint);
/// canvas.stroke_path(&mut path, &stroke_paint);
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct Paint {
    pub(crate) flavor: PaintFlavor,
    #[cfg_attr(feature = "serialization", serde(skip))]
    pub(crate) glyph_texture: GlyphTexture,
    pub(crate) shape_anti_alias: bool,
    pub(crate) stroke: StrokeSettings,
    #[cfg_attr(feature = "serialization", serde(skip))]
    pub(crate) font_ids: [Option<FontId>; 8],
    pub(crate) font_size: f32,
    pub(crate) letter_spacing: f32,
    pub(crate) text_baseline: Baseline,
    pub(crate) text_align: Align,
    pub(crate) fill_rule: FillRule,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            flavor: PaintFlavor::Color(Color::white()),
            glyph_texture: Default::default(),
            shape_anti_alias: true,
            stroke: StrokeSettings::default(),
            font_ids: Default::default(),
            font_size: 16.0,
            letter_spacing: 0.0,
            text_baseline: Default::default(),
            text_align: Default::default(),
            fill_rule: Default::default(),
        }
    }
}

impl Paint {
    /// Creates a new solid color paint
    pub fn color(color: Color) -> Self {
        Paint::with_flavor(PaintFlavor::Color(color))
    }

    fn with_flavor(flavor: PaintFlavor) -> Self {
        Paint {
            flavor,
            ..Default::default()
        }
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
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let image_id = canvas.load_image_file("examples/assets/rust-logo.png", ImageFlags::GENERATE_MIPMAPS).expect("Cannot create image");
    /// let fill_paint = Paint::image(image_id, 10.0, 10.0, 85.0, 85.0, 0.0, 1.0);
    ///
    /// let mut path = Path::new();
    /// path.rect(10.0, 10.0, 85.0, 85.0);
    /// canvas.fill_path(&mut path, &fill_paint);
    /// ```
    pub fn image(id: ImageId, cx: f32, cy: f32, width: f32, height: f32, angle: f32, alpha: f32) -> Self {
        Paint::with_flavor(PaintFlavor::Image {
            id,
            center: Position { x: cx, y: cy },
            width,
            height,
            angle,
            tint: Color::rgbaf(1.0, 1.0, 1.0, alpha),
        })
    }

    /// Like `image`, but allows for adding a tint, or a color which will transform each pixel's
    /// color via channel-wise multiplication.
    pub fn image_tint(id: ImageId, cx: f32, cy: f32, width: f32, height: f32, angle: f32, tint: Color) -> Self {
        Paint::with_flavor(PaintFlavor::Image {
            id,
            center: Position { x: cx, y: cy },
            width,
            height,
            angle,
            tint,
        })
    }

    /// Creates and returns a linear gradient paint.
    ///
    /// The gradient is transformed by the current transform when it is passed to fill_path() or stroke_path().
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::linear_gradient(0.0, 0.0, 0.0, 100.0, Color::rgba(255, 255, 255, 16), Color::rgba(0, 0, 0, 16));
    /// let mut path = Path::new();
    /// path.rounded_rect(0.0, 0.0, 100.0, 100.0, 5.0);
    /// canvas.fill_path(&mut path, &bg);
    /// ```
    pub fn linear_gradient(
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        start_color: Color,
        end_color: Color,
    ) -> Self {
        Paint::with_flavor(PaintFlavor::LinearGradient {
            start: Position { x: start_x, y: start_y },
            end: Position { x: end_x, y: end_y },
            colors: GradientColors::TwoStop { start_color, end_color },
        })
    }
    /// Creates and returns a linear gradient paint with two or more stops.
    ///
    /// The gradient is transformed by the current transform when it is passed to fill_path() or stroke_path().
    /// If a gradient has more than 24 stops, then only the first 24 stops will be used.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::linear_gradient_stops(
    ///    0.0, 0.0,
    ///    0.0, 100.0,
    ///    &[
    ///         (0.0, Color::rgba(255, 255, 255, 16)),
    ///         (0.5, Color::rgba(0, 0, 0, 16)),
    ///         (1.0, Color::rgba(255, 0, 0, 16))
    ///    ]);
    /// let mut path = Path::new();
    /// path.rounded_rect(0.0, 0.0, 100.0, 100.0, 5.0);
    /// canvas.fill_path(&mut path, &bg);
    /// ```
    pub fn linear_gradient_stops(start_x: f32, start_y: f32, end_x: f32, end_y: f32, stops: &[(f32, Color)]) -> Self {
        Paint::with_flavor(PaintFlavor::LinearGradient {
            start: Position { x: start_x, y: start_y },
            end: Position { x: end_x, y: end_y },
            colors: GradientColors::from_stops(stops),
        })
    }

    #[allow(clippy::too_many_arguments)]
    /// Creates and returns a box gradient.
    ///
    /// Box gradient is a feathered rounded rectangle, it is useful for rendering
    /// drop shadows or highlights for boxes. Parameters (x,y) define the top-left corner of the rectangle,
    /// (w,h) define the size of the rectangle, r defines the corner radius, and f feather. Feather defines how blurry
    /// the border of the rectangle is. Parameter inner_color specifies the inner color and outer_color the outer color of the gradient.
    /// The gradient is transformed by the current transform when it is passed to fill_path() or stroke_path().
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::box_gradient(
    ///    0.0,
    ///    0.0,
    ///    100.0,
    ///    100.0,
    ///    10.0,
    ///    10.0,
    ///    Color::rgba(0, 0, 0, 128),
    ///    Color::rgba(0, 0, 0, 0),
    /// );
    ///
    /// let mut path = Path::new();
    /// path.rounded_rect(0.0, 0.0, 100.0, 100.0, 5.0);
    /// canvas.fill_path(&mut path, &bg);
    /// ```
    pub fn box_gradient(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        feather: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        Paint::with_flavor(PaintFlavor::BoxGradient {
            pos: Position { x, y },
            width,
            height,
            radius,
            feather,
            colors: GradientColors::TwoStop {
                start_color: inner_color,
                end_color: outer_color,
            },
        })
    }

    /// Creates and returns a radial gradient.
    ///
    /// Parameters (cx,cy) specify the center, in_radius and out_radius specify
    /// the inner and outer radius of the gradient, inner_color specifies the start color and outer_color the end color.
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::radial_gradient(
    ///    50.0,
    ///    50.0,
    ///    18.0,
    ///    24.0,
    ///    Color::rgba(0, 0, 0, 128),
    ///    Color::rgba(0, 0, 0, 0),
    /// );
    ///
    /// let mut path = Path::new();
    /// path.circle(50.0, 50.0, 20.0);
    /// canvas.fill_path(&mut path, &bg);
    /// ```
    pub fn radial_gradient(
        cx: f32,
        cy: f32,
        in_radius: f32,
        out_radius: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        Paint::with_flavor(PaintFlavor::RadialGradient {
            center: Position { x: cx, y: cy },
            in_radius,
            out_radius,
            colors: GradientColors::TwoStop {
                start_color: inner_color,
                end_color: outer_color,
            },
        })
    }

    /// Creates and returns a multi-stop radial gradient.
    ///
    /// Parameters (cx,cy) specify the center, in_radius and out_radius specify the inner and outer radius of the gradient,
    /// colors specifies a list of color stops with offsets. The first offset should be 0.0 and the last offset should be 1.0.
    /// If a gradient has more than 24 stops, then only the first 24 stops will be used.
    ///
    /// The gradient is transformed by the current transform when it is passed to fill_paint() or stroke_paint().
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::radial_gradient_stops(
    ///    50.0,
    ///    50.0,
    ///    18.0,
    ///    24.0,
    ///    &[
    ///         (0.0, Color::rgba(0, 0, 0, 128)),
    ///         (0.5, Color::rgba(0, 0, 128, 128)),
    ///         (1.0, Color::rgba(0, 128, 0, 128))
    ///    ]
    /// );
    ///
    /// let mut path = Path::new();
    /// path.circle(50.0, 50.0, 20.0);
    /// canvas.fill_path(&mut path, &bg);
    /// ```
    pub fn radial_gradient_stops(cx: f32, cy: f32, in_radius: f32, out_radius: f32, stops: &[(f32, Color)]) -> Self {
        Paint::with_flavor(PaintFlavor::RadialGradient {
            center: Position { x: cx, y: cy },
            in_radius,
            out_radius,
            colors: GradientColors::from_stops(stops),
        })
    }

    /// Creates a new solid color paint
    pub fn set_color(&mut self, color: Color) {
        self.flavor = PaintFlavor::Color(color);
    }

    /// Returns the paint with a new solid color set to the specified value.
    pub fn with_color(mut self, color: Color) -> Self {
        self.set_color(color);
        self
    }

    /// Set an alpha mask or color glyph texture; this is only used by draw_triangles which is used for text.
    // This is scoped to crate visibility because fill_path and stroke_path don't propagate
    // the alpha mask (so nothing draws), and the texture coordinates are used for antialiasing
    // when path drawing.
    pub(crate) fn set_glyph_texture(&mut self, texture: GlyphTexture) {
        self.glyph_texture = texture;
    }

    /// Returns boolean if the shapes drawn with this paint will be antialiased.
    pub fn anti_alias(&self) -> bool {
        self.shape_anti_alias
    }

    /// Sets whether shapes drawn with this paint will be anti aliased. Enabled by default.
    pub fn set_anti_alias(&mut self, value: bool) {
        self.shape_anti_alias = value;
    }

    /// Returns the paint with anti alias set to the specified value.
    pub fn with_anti_alias(mut self, value: bool) -> Self {
        self.set_anti_alias(value);
        self
    }

    /// True if this paint uses higher quality stencil strokes.
    pub fn stencil_strokes(&self) -> bool {
        self.stroke.stencil_strokes
    }

    /// Sets whether to use higher quality stencil strokes.
    pub fn set_stencil_strokes(&mut self, value: bool) {
        self.stroke.stencil_strokes = value;
    }

    /// Returns the paint with stencil strokes set to the specified value.
    pub fn with_stencil_strokes(mut self, value: bool) -> Self {
        self.set_stencil_strokes(value);
        self
    }

    /// Returns the current line width.
    pub fn line_width(&self) -> f32 {
        self.stroke.line_width
    }

    /// Sets the line width for shapes stroked with this paint.
    pub fn set_line_width(&mut self, width: f32) {
        self.stroke.line_width = width;
    }

    /// Returns the paint with line width set to the specified value.
    pub fn with_line_width(mut self, width: f32) -> Self {
        self.set_line_width(width);
        self
    }

    /// Getter for the miter limit
    pub fn miter_limit(&self) -> f32 {
        self.stroke.miter_limit
    }

    /// Sets the limit at which a sharp corner is drawn beveled.
    ///
    /// If the miter at a corner exceeds this limit, LineJoin is replaced with LineJoin::Bevel.
    pub fn set_miter_limit(&mut self, limit: f32) {
        self.stroke.miter_limit = limit;
    }

    /// Returns the paint with the miter limit set to the specified value.
    pub fn with_miter_limit(mut self, limit: f32) -> Self {
        self.set_miter_limit(limit);
        self
    }

    /// Returns the current start line cap for this paint.
    pub fn line_cap_start(&self) -> LineCap {
        self.stroke.line_cap_start
    }

    /// Returns the current start line cap for this paint.
    pub fn line_cap_end(&self) -> LineCap {
        self.stroke.line_cap_end
    }

    /// Sets how the start and end of the line (cap) is drawn
    ///
    /// By default it's set to LineCap::Butt
    pub fn set_line_cap(&mut self, cap: LineCap) {
        self.stroke.line_cap_start = cap;
        self.stroke.line_cap_end = cap;
    }

    /// Returns the paint with line cap set to the specified value.
    pub fn with_line_cap(mut self, cap: LineCap) -> Self {
        self.set_line_cap(cap);
        self
    }

    /// Sets how the beggining cap of the line is drawn
    ///
    /// By default it's set to LineCap::Butt
    pub fn set_line_cap_start(&mut self, cap: LineCap) {
        self.stroke.line_cap_start = cap;
    }

    /// Returns the paint with the beginning cap of the line set to the specified value.
    pub fn with_line_cap_start(mut self, cap: LineCap) -> Self {
        self.set_line_cap_start(cap);
        self
    }

    /// Sets how the end cap of the line is drawn
    ///
    /// By default it's set to LineCap::Butt
    pub fn set_line_cap_end(&mut self, cap: LineCap) {
        self.stroke.line_cap_end = cap;
    }

    /// Returns the paint with the beginning cap of the line set to the specified value.
    pub fn with_line_cap_end(mut self, cap: LineCap) -> Self {
        self.set_line_cap_end(cap);
        self
    }

    /// Returns the current line join for this paint.
    pub fn line_join(&self) -> LineJoin {
        self.stroke.line_join
    }

    /// Sets how sharp path corners are drawn.
    ///
    /// By default it's set to LineJoin::Miter
    pub fn set_line_join(&mut self, join: LineJoin) {
        self.stroke.line_join = join;
    }

    /// Returns the paint with the line join set to the specified value.
    pub fn with_line_join(mut self, join: LineJoin) -> Self {
        self.set_line_join(join);
        self
    }

    pub fn set_font(&mut self, font_ids: &[FontId]) {
        self.font_ids = Default::default();

        for (i, id) in font_ids.iter().take(8).enumerate() {
            self.font_ids[i] = Some(*id);
        }
    }

    /// Returns the paint with the font set to the specified value.
    pub fn with_font(mut self, font_ids: &[FontId]) -> Self {
        self.set_font(font_ids);
        self
    }

    /// Returns the current font size
    ///
    /// Only has effect on canvas text operations
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// Sets the font size.
    ///
    /// Only has effect on canvas text operations
    pub fn set_font_size(&mut self, size: f32) {
        self.font_size = size;
    }

    /// Returns the paint with the font size set to the specified value.
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.set_font_size(size);
        self
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

    /// Returns the paint with the letter spacing set to the specified value.
    pub fn with_letter_spacing(mut self, spacing: f32) -> Self {
        self.set_letter_spacing(spacing);
        self
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

    /// Returns the paint with the text vertical alignment set to the specified value.
    pub fn with_text_baseline(mut self, align: Baseline) -> Self {
        self.set_text_baseline(align);
        self
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

    /// Returns the paint with the text horizontal alignment set to the specified value.
    pub fn with_text_align(mut self, align: Align) -> Self {
        self.set_text_align(align);
        self
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

    /// Returns the paint with the rule for filling a path set to the specified value.
    pub fn with_fill_rule(mut self, rule: FillRule) -> Self {
        self.set_fill_rule(rule);
        self
    }
}
