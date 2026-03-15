// TODO: prefix paint creation functions with make_ or new_
// so that they are easier to find when autocompleting

use std::rc::Rc;

use crate::{Align, Baseline, Color, FontId, ImageId, LineCap, LineJoin, geometry::Position};

#[derive(Copy, Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GradientStop(pub f32, pub Color);

// We use MultiStopGradient as a key since we cache them. We either need
// to define Hash (for HashMap) or Ord for (BTreeMap).
impl Eq for GradientStop {}
impl Ord for GradientStop {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .total_cmp(&other.0)
            .then(self.1.r.total_cmp(&other.1.r))
            .then(self.1.g.total_cmp(&other.1.g))
            .then(self.1.b.total_cmp(&other.1.b))
            .then(self.1.a.total_cmp(&other.1.a))
    }
}

impl PartialOrd for GradientStop {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MultiStopGradient {
    shared_stops: Rc<[GradientStop]>,
    tint: f32,
}

impl MultiStopGradient {
    pub(crate) fn get(&self, index: usize) -> GradientStop {
        let mut stop = self
            .shared_stops
            .get(index)
            .copied()
            .unwrap_or_else(|| GradientStop(2.0, Color::black()));

        stop.1.a *= self.tint;
        stop
    }

    pub(crate) fn pairs(&self) -> impl Iterator<Item = [GradientStop; 2]> + '_ {
        self.shared_stops.as_ref().windows(2).map(move |pair| {
            let mut stops = [pair[0], pair[1]];
            stops[0].1.a *= self.tint;
            stops[1].1.a *= self.tint;
            stops
        })
    }
}

impl Eq for MultiStopGradient {}

impl PartialOrd for MultiStopGradient {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MultiStopGradient {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.shared_stops
            .cmp(&other.shared_stops)
            .then(self.tint.total_cmp(&other.tint))
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum GradientColors {
    TwoStop { start_color: Color, end_color: Color },
    MultiStop { stops: MultiStopGradient },
}
impl GradientColors {
    fn mul_alpha(&mut self, a: f32) {
        match self {
            Self::TwoStop { start_color, end_color } => {
                start_color.a *= a;
                end_color.a *= a;
            }
            Self::MultiStop { stops, .. } => {
                stops.tint *= a;
            }
        }
    }
    fn from_stops<Stops>(stops: Stops) -> Self
    where
        Stops: IntoIterator<Item = (f32, Color)>,
    {
        let mut stops = stops.into_iter();
        let Some(first_stop) = stops.next() else {
            // No stops, we use black.
            return Self::TwoStop {
                start_color: Color::black(),
                end_color: Color::black(),
            };
        };
        let Some(second_stop) = stops.next() else {
            // One stop devolves to a solid color fill (but using the gradient shader variation).
            return Self::TwoStop {
                start_color: first_stop.1,
                end_color: first_stop.1,
            };
        };

        let maybe_third_stop = stops.next();

        if maybe_third_stop.is_none() && first_stop.0 <= 0.0 && second_stop.0 >= 1.0 {
            // Two stops takes the classic gradient path, so long as the stop positions are at
            // the extents (if the stop positions are inset then we'll fill to them).
            return Self::TwoStop {
                start_color: first_stop.1,
                end_color: second_stop.1,
            };
        }

        // Actual multistop gradient. We copy out the stops and then use a stop with a
        // position > 1.0 as a sentinel. GradientStore ignores stop positions > 1.0
        // when synthesizing the gradient texture.
        let out_stops = [first_stop, second_stop]
            .into_iter()
            .chain(maybe_third_stop)
            .chain(stops)
            .map(|(stop, color)| GradientStop(stop, color))
            .collect();
        Self::MultiStop {
            stops: MultiStopGradient {
                shared_stops: out_stops,
                tint: 1.0,
            },
        }
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PaintFlavor {
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
    ConicGradient {
        center: Position,
        colors: GradientColors,
    },
}

// Convenience method to fetch the GradientColors out of a PaintFlavor
impl PaintFlavor {
    pub(crate) fn mul_alpha(&mut self, a: f32) {
        match self {
            Self::Color(color) => {
                color.a *= a;
            }
            Self::Image { tint, .. } => {
                tint.a *= a;
            }
            Self::LinearGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            Self::BoxGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            Self::RadialGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            Self::ConicGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
        }
    }

    pub(crate) fn gradient_colors(&self) -> Option<&GradientColors> {
        match self {
            Self::LinearGradient { colors, .. } => Some(colors),
            Self::BoxGradient { colors, .. } => Some(colors),
            Self::RadialGradient { colors, .. } => Some(colors),
            Self::ConicGradient { colors, .. } => Some(colors),
            _ => None,
        }
    }

    /// Returns true if this paint is an untransformed image paint without anti-aliasing at the edges in case of a fill
    pub(crate) fn is_straight_tinted_image(&self, shape_anti_alias: bool) -> bool {
        matches!(self, &Self::Image { angle, .. } if angle == 0.0 && !shape_anti_alias)
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum GlyphTexture {
    #[default]
    None,
    AlphaMask(ImageId),
    ColorTexture(ImageId),
}

impl GlyphTexture {
    pub(crate) fn image_id(&self) -> Option<ImageId> {
        match self {
            Self::None => None,
            Self::AlphaMask(image_id) | Self::ColorTexture(image_id) => Some(*image_id),
        }
    }
}

/// Settings controlling how strokes are rendered.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct StrokeSettings {
    pub(crate) stencil_strokes: bool,
    pub(crate) miter_limit: f32,
    pub(crate) line_width: f32,
    pub(crate) line_cap_start: LineCap,
    pub(crate) line_cap_end: LineCap,
    pub(crate) line_join: LineJoin,
}

impl StrokeSettings {
    /// Creates new stroke settings with the specified line width.
    pub fn new(line_width: f32) -> Self {
        Self {
            line_width,
            ..Default::default()
        }
    }

    /// Returns whether higher quality stencil strokes are used.
    #[inline]
    pub fn stencil_strokes(&self) -> bool {
        self.stencil_strokes
    }

    /// Returns the settings with stencil strokes set to the specified value.
    #[inline]
    pub fn with_stencil_strokes(mut self, value: bool) -> Self {
        self.stencil_strokes = value;
        self
    }

    /// Returns the current line width.
    #[inline]
    pub fn line_width(&self) -> f32 {
        self.line_width
    }

    /// Returns the settings with line width set to the specified value.
    #[inline]
    pub fn with_line_width(mut self, width: f32) -> Self {
        self.line_width = width;
        self
    }

    /// Returns the current miter limit.
    #[inline]
    pub fn miter_limit(&self) -> f32 {
        self.miter_limit
    }

    /// Returns the settings with the miter limit set to the specified value.
    #[inline]
    pub fn with_miter_limit(mut self, limit: f32) -> Self {
        self.miter_limit = limit;
        self
    }

    /// Returns the current start line cap.
    #[inline]
    pub fn line_cap_start(&self) -> LineCap {
        self.line_cap_start
    }

    /// Returns the current end line cap.
    #[inline]
    pub fn line_cap_end(&self) -> LineCap {
        self.line_cap_end
    }

    /// Returns the settings with the line cap set for both start and end.
    #[inline]
    pub fn with_line_cap(mut self, cap: LineCap) -> Self {
        self.line_cap_start = cap;
        self.line_cap_end = cap;
        self
    }

    /// Returns the settings with the start line cap set to the specified value.
    #[inline]
    pub fn with_line_cap_start(mut self, cap: LineCap) -> Self {
        self.line_cap_start = cap;
        self
    }

    /// Returns the settings with the end line cap set to the specified value.
    #[inline]
    pub fn with_line_cap_end(mut self, cap: LineCap) -> Self {
        self.line_cap_end = cap;
        self
    }

    /// Returns the current line join.
    #[inline]
    pub fn line_join(&self) -> LineJoin {
        self.line_join
    }

    /// Returns the settings with the line join set to the specified value.
    #[inline]
    pub fn with_line_join(mut self, join: LineJoin) -> Self {
        self.line_join = join;
        self
    }
}

impl Default for StrokeSettings {
    fn default() -> Self {
        Self {
            stencil_strokes: true,
            miter_limit: 10.0,
            line_width: 1.0,
            line_cap_start: LineCap::default(),
            line_cap_end: LineCap::default(),
            line_join: LineJoin::default(),
        }
    }
}

/// Settings controlling text rendering, such as font, size, spacing, and alignment.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TextSettings {
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) font_ids: [Option<FontId>; 8],
    /// Font size in pixels.
    pub font_size: f32,
    /// Additional horizontal spacing between characters.
    pub letter_spacing: f32,
    /// Vertical baseline alignment.
    pub baseline: Baseline,
    /// Horizontal text alignment.
    pub align: Align,
}

impl TextSettings {
    /// Creates new text settings with the given fonts and font size.
    pub fn new(font_ids: &[FontId], font_size: f32) -> Self {
        let mut ids: [Option<FontId>; 8] = Default::default();
        for (i, id) in font_ids.iter().take(8).enumerate() {
            ids[i] = Some(*id);
        }
        Self {
            font_ids: ids,
            font_size,
            ..Default::default()
        }
    }

    /// Returns the settings with letter spacing set to the specified value.
    pub fn with_letter_spacing(mut self, spacing: f32) -> Self {
        self.letter_spacing = spacing;
        self
    }

    /// Returns the settings with the baseline set to the specified value.
    pub fn with_baseline(mut self, baseline: Baseline) -> Self {
        self.baseline = baseline;
        self
    }

    /// Returns the settings with horizontal alignment set to the specified value.
    pub fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    #[inline]
    pub(crate) fn scaled(&self, scale: f32) -> Self {
        Self {
            font_size: self.font_size * scale,
            letter_spacing: self.letter_spacing * scale,
            ..*self
        }
    }
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            font_ids: Default::default(),
            font_size: 16.0,
            letter_spacing: 0.0,
            baseline: Baseline::default(),
            align: Align::default(),
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
/// use femtovg::{Paint, Path, Color, FillRule, Canvas, renderer::Void, StrokeSettings};
///
/// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
///
/// let fill_paint = Paint::color(Color::hex("454545"));
/// let stroke_paint = Paint::color(Color::hex("bababa"));
/// let stroke = StrokeSettings::new(4.0);
///
/// let mut path = Path::new();
/// path.rounded_rect([10.0, 10.0], [100.0, 100.0], 20.0);
/// canvas.fill_path(&path, &fill_paint, FillRule::default());
/// canvas.stroke_path(&path, &stroke_paint, &stroke);
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Paint {
    pub(crate) flavor: PaintFlavor,
    pub(crate) shape_anti_alias: bool,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            flavor: PaintFlavor::Color(Color::white()),
            shape_anti_alias: true,
        }
    }
}

impl Paint {
    /// Creates a new solid color paint
    pub fn color(color: Color) -> Self {
        Self::with_flavor(PaintFlavor::Color(color))
    }

    fn with_flavor(flavor: PaintFlavor) -> Self {
        Self {
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
    /// use femtovg::{Paint, Path, Color, FillRule, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let image_id = canvas.load_image_file("examples/assets/rust-logo.png", ImageFlags::GENERATE_MIPMAPS).expect("Cannot create image");
    /// let fill_paint = Paint::image(image_id, [10.0, 10.0], [85.0, 85.0], 0.0, 1.0);
    ///
    /// let mut path = Path::new();
    /// path.rect([10.0, 10.0], [85.0, 85.0]);
    /// canvas.fill_path(&path, &fill_paint, FillRule::default());
    /// ```
    pub fn image(id: ImageId, pos: impl Into<[f32; 2]>, size: impl Into<[f32; 2]>, angle: f32, alpha: f32) -> Self {
        let [x, y] = pos.into();
        let [width, height] = size.into();
        Self::with_flavor(PaintFlavor::Image {
            id,
            center: Position { x, y },
            width,
            height,
            angle,
            tint: Color::rgbaf(1.0, 1.0, 1.0, alpha),
        })
    }

    /// Like `image`, but allows for adding a tint, or a color which will transform each pixel's
    /// color via channel-wise multiplication.
    pub fn image_tint(
        id: ImageId,
        pos: impl Into<[f32; 2]>,
        size: impl Into<[f32; 2]>,
        angle: f32,
        tint: Color,
    ) -> Self {
        let [x, y] = pos.into();
        let [width, height] = size.into();
        Self::with_flavor(PaintFlavor::Image {
            id,
            center: Position { x, y },
            width,
            height,
            angle,
            tint,
        })
    }

    /// Creates and returns a linear gradient paint.
    ///
    /// The gradient is transformed by the current transform when it is passed to `fill_path()` or `stroke_path()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, FillRule, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::linear_gradient([0.0, 0.0], [0.0, 100.0], Color::rgba(255, 255, 255, 16), Color::rgba(0, 0, 0, 16));
    /// let mut path = Path::new();
    /// path.rounded_rect([0.0, 0.0], [100.0, 100.0], 5.0);
    /// canvas.fill_path(&path, &bg, FillRule::default());
    /// ```
    pub fn linear_gradient(
        start: impl Into<[f32; 2]>,
        end: impl Into<[f32; 2]>,
        start_color: Color,
        end_color: Color,
    ) -> Self {
        let [start_x, start_y] = start.into();
        let [end_x, end_y] = end.into();
        Self::with_flavor(PaintFlavor::LinearGradient {
            start: Position { x: start_x, y: start_y },
            end: Position { x: end_x, y: end_y },
            colors: GradientColors::TwoStop { start_color, end_color },
        })
    }
    /// Creates and returns a linear gradient paint with two or more stops.
    ///
    /// The gradient is transformed by the current transform when it is passed to `fill_path()` or `stroke_path()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, FillRule, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::linear_gradient_stops(
    ///    [0.0, 0.0],
    ///    [0.0, 100.0],
    ///    [
    ///         (0.0, Color::rgba(255, 255, 255, 16)),
    ///         (0.5, Color::rgba(0, 0, 0, 16)),
    ///         (1.0, Color::rgba(255, 0, 0, 16))
    ///    ]);
    /// let mut path = Path::new();
    /// path.rounded_rect([0.0, 0.0], [100.0, 100.0], 5.0);
    /// canvas.fill_path(&path, &bg, FillRule::default());
    /// ```
    pub fn linear_gradient_stops(
        start: impl Into<[f32; 2]>,
        end: impl Into<[f32; 2]>,
        stops: impl IntoIterator<Item = (f32, Color)>,
    ) -> Self {
        let [start_x, start_y] = start.into();
        let [end_x, end_y] = end.into();
        Self::with_flavor(PaintFlavor::LinearGradient {
            start: Position { x: start_x, y: start_y },
            end: Position { x: end_x, y: end_y },
            colors: GradientColors::from_stops(stops),
        })
    }

    /// Creates and returns a box gradient.
    ///
    /// Box gradient is a feathered rounded rectangle, it is useful for rendering
    /// drop shadows or highlights for boxes. Parameters (x,y) define the top-left corner of the rectangle,
    /// (w,h) define the size of the rectangle, r defines the corner radius, and f feather. Feather defines how blurry
    /// the border of the rectangle is. Parameter `inner_color` specifies the inner color and `outer_color` the outer color of the gradient.
    /// The gradient is transformed by the current transform when it is passed to `fill_path()` or `stroke_path()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, FillRule, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::box_gradient(
    ///    [0.0, 0.0],
    ///    [100.0, 100.0],
    ///    10.0,
    ///    10.0,
    ///    Color::rgba(0, 0, 0, 128),
    ///    Color::rgba(0, 0, 0, 0),
    /// );
    ///
    /// let mut path = Path::new();
    /// path.rounded_rect([0.0, 0.0], [100.0, 100.0], 5.0);
    /// canvas.fill_path(&path, &bg, FillRule::default());
    /// ```
    pub fn box_gradient(
        pos: impl Into<[f32; 2]>,
        size: impl Into<[f32; 2]>,
        radius: f32,
        feather: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        let [x, y] = pos.into();
        let [width, height] = size.into();
        Self::with_flavor(PaintFlavor::BoxGradient {
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
    /// Parameters (`cx`,`cy`) specify the center, `in_radius` and `out_radius` specify
    /// the inner and outer radius of the gradient, `inner_color` specifies the start color and `outer_color` the end color.
    /// The gradient is transformed by the current transform when it is passed to `fill_paint()` or `stroke_paint()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, FillRule, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::radial_gradient(
    ///    [50.0, 50.0],
    ///    18.0,
    ///    24.0,
    ///    Color::rgba(0, 0, 0, 128),
    ///    Color::rgba(0, 0, 0, 0),
    /// );
    ///
    /// let mut path = Path::new();
    /// path.circle([50.0, 50.0], 20.0);
    /// canvas.fill_path(&path, &bg, FillRule::default());
    /// ```
    pub fn radial_gradient(
        center: impl Into<[f32; 2]>,
        in_radius: f32,
        out_radius: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        let [cx, cy] = center.into();
        Self::with_flavor(PaintFlavor::RadialGradient {
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
    /// Parameters (`cx`,`cy`) specify the center, `in_radius` and `out_radius` specify the inner and outer radius of the gradient,
    /// colors specifies a list of color stops with offsets. The first offset should be 0.0 and the last offset should be 1.0.
    ///
    /// The gradient is transformed by the current transform when it is passed to `fill_paint()` or `stroke_paint()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, FillRule, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::radial_gradient_stops(
    ///    [50.0, 50.0],
    ///    18.0,
    ///    24.0,
    ///    [
    ///         (0.0, Color::rgba(0, 0, 0, 128)),
    ///         (0.5, Color::rgba(0, 0, 128, 128)),
    ///         (1.0, Color::rgba(0, 128, 0, 128))
    ///    ]
    /// );
    ///
    /// let mut path = Path::new();
    /// path.circle([50.0, 50.0], 20.0);
    /// canvas.fill_path(&path, &bg, FillRule::default());
    /// ```
    pub fn radial_gradient_stops(
        center: impl Into<[f32; 2]>,
        in_radius: f32,
        out_radius: f32,
        stops: impl IntoIterator<Item = (f32, Color)>,
    ) -> Self {
        let [cx, cy] = center.into();
        Self::with_flavor(PaintFlavor::RadialGradient {
            center: Position { x: cx, y: cy },
            in_radius,
            out_radius,
            colors: GradientColors::from_stops(stops),
        })
    }

    /// Creates and returns a multi-stop conic gradient.
    ///
    /// Parameters (`cx`,`cy`) specify the center.
    pub fn conic_gradient_stops(center: impl Into<[f32; 2]>, stops: impl IntoIterator<Item = (f32, Color)>) -> Self {
        let [cx, cy] = center.into();
        Self::with_flavor(PaintFlavor::ConicGradient {
            center: Position { x: cx, y: cy },
            colors: GradientColors::from_stops(stops),
        })
    }

    /// Sets the color of the paint.
    pub fn set_color(&mut self, color: Color) {
        self.flavor = PaintFlavor::Color(color);
    }

    /// Returns the paint with the color set to the specified value.
    #[inline]
    pub fn with_color(mut self, color: Color) -> Self {
        self.set_color(color);
        self
    }

    /// Returns the current anti-alias setting.
    #[inline]
    pub fn anti_alias(&self) -> bool {
        self.shape_anti_alias
    }

    /// Sets whether shapes drawn with this paint will be anti-aliased.
    #[inline]
    pub fn set_anti_alias(&mut self, value: bool) {
        self.shape_anti_alias = value;
    }

    /// Returns the paint with anti-alias set to the specified value.
    #[inline]
    pub fn with_anti_alias(mut self, value: bool) -> Self {
        self.set_anti_alias(value);
        self
    }
}
