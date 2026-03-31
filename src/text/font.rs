use fnv::FnvHashMap;
use std::cell::{Ref, RefCell};
use std::collections::hash_map::Entry;
use std::fmt;
#[cfg(all(feature = "swash", not(feature = "textlayout")))]
use std::rc::Rc;
#[cfg(feature = "textlayout")]
use ttf_parser::{Face as TtfFont, GlyphId};

use crate::{paint::FontVariations, ErrorKind, Path};

/// Abstraction over the parsed font face, so callers don't need cfg blocks.
/// With `textlayout`, this wraps a `ttf_parser::Face`.
/// Otherwise, this is a zero-sized type.
#[cfg(feature = "textlayout")]
pub(crate) struct FontFaceRef<'a>(pub(crate) ttf_parser::Face<'a>);

#[cfg(not(feature = "textlayout"))]
pub(crate) struct FontFaceRef<'a>(std::marker::PhantomData<&'a ()>);

/// Information about a font variation axis (e.g. weight, width, optical size).
///
/// Returned by [`Canvas::font_variation_axes`](crate::Canvas::font_variation_axes)
/// in the order they appear in the font's OpenType `fvar` table. The position
/// of each axis in that vector determines the index of the corresponding
/// normalized coordinate (`i16` in F2DOT14 format) when calling
/// [`Canvas::fill_glyph_run`](crate::Canvas::fill_glyph_run) or
/// [`Canvas::stroke_glyph_run`](crate::Canvas::stroke_glyph_run).
#[derive(Clone, Debug)]
pub struct VariationAxisInfo {
    /// Four-byte axis tag (e.g. `*b"wght"` for weight).
    pub tag: [u8; 4],
    /// Minimum value for this axis.
    pub min_value: f32,
    /// Default value for this axis.
    pub def_value: f32,
    /// Maximum value for this axis.
    pub max_value: f32,
    /// Name table ID for this axis's name.
    pub name_id: u16,
    /// Whether this axis is hidden from the user.
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct GlyphMetrics {
    pub width: f32,
    pub height: f32,
    pub bearing_x: f32,
    pub bearing_y: f32,
}

#[derive(Debug)]
pub struct Glyph {
    pub path: Option<Path>, // None means render as image
    pub metrics: GlyphMetrics,
}

pub enum GlyphRendering<'a> {
    RenderAsPath(Ref<'a, Path>),
    #[cfg(feature = "image-loading")]
    RenderAsImage(image::DynamicImage),
}

#[derive(Copy, Clone, Default, Debug)]
struct FontFlags(u8);

// TODO: underline, strikeout, subscript, superscript metrics
impl FontFlags {
    fn new(regular: bool, italic: bool, bold: bool, oblique: bool, variable: bool) -> Self {
        let mut flags = 0;
        if regular {
            flags |= 0x1;
        }
        if italic {
            flags |= 0x2;
        }
        if bold {
            flags |= 0x4;
        }
        if oblique {
            flags |= 0x8;
        }
        if variable {
            flags |= 0x10;
        }
        Self(flags)
    }

    #[inline]
    fn regular(&self) -> bool {
        self.0 & 0x1 > 0
    }

    #[inline]
    fn italic(&self) -> bool {
        self.0 & 0x2 > 0
    }

    #[inline]
    fn bold(&self) -> bool {
        self.0 & 0x4 > 0
    }

    #[inline]
    fn oblique(&self) -> bool {
        self.0 & 0x8 > 0
    }

    #[inline]
    fn variable(&self) -> bool {
        self.0 & 0x10 > 0
    }
}

/// Information about a font.
#[derive(Copy, Clone, Default, Debug)]
pub struct FontMetrics {
    ascender: f32,
    descender: f32,
    height: f32,
    flags: FontFlags,
    weight: u16,
    width: u16,
}

impl FontMetrics {
    fn scale(&mut self, scale: f32) {
        self.ascender *= scale;
        self.descender *= scale;
        self.height *= scale;
    }

    /// Returns the distance from the baseline to the top of the highest glyph.
    pub fn ascender(&self) -> f32 {
        self.ascender
    }

    /// Returns the distance from the baseline to the bottom of the lowest descenders on the glyphs.
    pub fn descender(&self) -> f32 {
        self.descender
    }

    /// Returns the height of the font.
    pub fn height(&self) -> f32 {
        self.height.round()
    }

    /// Returns if the font is regular.
    pub fn regular(&self) -> bool {
        self.flags.regular()
    }

    /// Returns if the font is italic.
    pub fn italic(&self) -> bool {
        self.flags.italic()
    }

    /// Returns if the font is bold.
    pub fn bold(&self) -> bool {
        self.flags.bold()
    }

    /// Returns if the font is oblique.
    pub fn oblique(&self) -> bool {
        self.flags.oblique()
    }

    /// Returns if the font is a variable font.
    pub fn variable(&self) -> bool {
        self.flags.variable()
    }

    /// Returns the weight of the font.
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// Returns the width of the font.
    pub fn width(&self) -> u16 {
        self.width
    }
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("data", &format_args!(".."))
            .field("face_index", &self.face_index)
            .field("units_per_em", &self.units_per_em)
            .field("metrics", &self.metrics)
            .field("glyphs", &self.glyphs)
            .finish()
    }
}

/// Key for the glyph cache: (glyph_id, normalized_coords_hash).
/// The hash covers all normalized variation coordinates, or 0 for defaults.
type GlyphCacheKey = (u16, u64);

pub struct Font {
    data: Box<dyn AsRef<[u8]>>,
    face_index: u32,
    units_per_em: u16,
    metrics: FontMetrics,
    glyphs: RefCell<FnvHashMap<GlyphCacheKey, Glyph>>,
    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    swash_scale_context: Rc<RefCell<swash::scale::ScaleContext>>,
}

impl Font {
    #[cfg(feature = "textlayout")]
    pub fn new_with_data<T: AsRef<[u8]> + 'static>(
        data: T,
        face_index: u32,
        _text_context: &super::TextContextImpl,
    ) -> Result<Self, ErrorKind> {
        let ttf_font = TtfFont::parse(data.as_ref(), face_index).map_err(|_| ErrorKind::FontParseError)?;

        let units_per_em = ttf_font.units_per_em();

        let metrics = FontMetrics {
            ascender: ttf_font.ascender() as f32,
            descender: ttf_font.descender() as f32,
            height: ttf_font.height() as f32,
            flags: FontFlags::new(
                ttf_font.is_regular(),
                ttf_font.is_italic(),
                ttf_font.is_bold(),
                ttf_font.is_oblique(),
                ttf_font.is_variable(),
            ),
            width: ttf_font.width().to_number(),
            weight: ttf_font.weight().to_number(),
        };

        Ok(Self {
            data: Box::new(data),
            face_index,
            units_per_em,
            metrics,
            glyphs: RefCell::default(),
        })
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub fn new_with_data<T: AsRef<[u8]> + 'static>(
        data: T,
        face_index: u32,
        text_context: &super::TextContextImpl,
    ) -> Result<Self, ErrorKind> {
        let font_ref =
            swash::FontRef::from_index(data.as_ref(), face_index as usize).ok_or(ErrorKind::FontParseError)?;

        let swash_metrics = font_ref.metrics(&[]);
        let attrs = font_ref.attributes();

        let units_per_em = swash_metrics.units_per_em;

        let weight = attrs.weight().0;
        let stretch = attrs.stretch();
        let style = attrs.style();

        let is_bold = weight >= 700;
        let is_italic = matches!(style, swash::Style::Italic);
        let is_oblique = matches!(style, swash::Style::Oblique(_));
        let is_variable = font_ref.variations().next().is_some();

        let is_regular = weight == 400 && matches!(style, swash::Style::Normal) && stretch == swash::Stretch::NORMAL;

        // Map swash::Stretch to usWidthClass values as per
        // https://learn.microsoft.com/en-us/typography/opentype/spec/os2#uswidthclass
        let width: u16 = match stretch {
            swash::Stretch::ULTRA_CONDENSED => 1,
            swash::Stretch::EXTRA_CONDENSED => 2,
            swash::Stretch::CONDENSED => 3,
            swash::Stretch::SEMI_CONDENSED => 4,
            swash::Stretch::NORMAL => 5,
            swash::Stretch::SEMI_EXPANDED => 6,
            swash::Stretch::EXPANDED => 7,
            swash::Stretch::EXTRA_EXPANDED => 8,
            swash::Stretch::ULTRA_EXPANDED => 9,
            _ => 5,
        };

        let metrics = FontMetrics {
            ascender: swash_metrics.ascent,
            descender: -swash_metrics.descent,
            // swash ascent and descent are both positive (distance from baseline),
            // unlike ttf-parser where descent is negative, so this is a sum not a difference.
            height: swash_metrics.ascent + swash_metrics.descent + swash_metrics.leading,
            flags: FontFlags::new(is_regular, is_italic, is_bold, is_oblique, is_variable),
            weight,
            width,
        };

        Ok(Self {
            data: Box::new(data),
            face_index,
            units_per_em,
            metrics,
            glyphs: RefCell::default(),
            swash_scale_context: text_context.swash_scale_context(),
        })
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub fn new_with_data<T: AsRef<[u8]> + 'static>(
        _data: T,
        _face_index: u32,
        _text_context: &super::TextContextImpl,
    ) -> Result<Self, ErrorKind> {
        Err(ErrorKind::FontParseError)
    }

    #[allow(dead_code)]
    pub fn data(&self) -> &[u8] {
        (*self.data).as_ref()
    }

    #[allow(dead_code)]
    pub fn face_index(&self) -> u32 {
        self.face_index
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn face_ref(&self) -> FontFaceRef<'_> {
        FontFaceRef(ttf_parser::Face::parse(self.data.as_ref().as_ref(), self.face_index).unwrap())
    }

    #[cfg(not(feature = "textlayout"))]
    pub(crate) fn face_ref(&self) -> FontFaceRef<'_> {
        FontFaceRef(std::marker::PhantomData)
    }

    #[cfg(feature = "swash")]
    pub(crate) fn swash_font_ref(&self) -> Option<swash::FontRef<'_>> {
        swash::FontRef::from_index(self.data.as_ref().as_ref(), self.face_index as usize)
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    fn swash_scale_context(&self) -> &RefCell<swash::scale::ScaleContext> {
        &self.swash_scale_context
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn face_ref_with_variations(&self, variations: &FontVariations) -> FontFaceRef<'_> {
        let mut face = self.face_ref();
        for (tag, value) in variations.iter() {
            face.0.set_variation(ttf_parser::Tag(tag), value);
        }
        face
    }

    #[cfg(not(feature = "textlayout"))]
    #[allow(dead_code)]
    pub(crate) fn face_ref_with_variations(&self, _variations: &FontVariations) -> FontFaceRef<'_> {
        // Without ttf-parser, face is a phantom type. Variations are applied
        // through the swash scaler in the glyph() method instead.
        self.face_ref()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn face_ref_with_normalized_coords(&self, coords: &[i16]) -> FontFaceRef<'_> {
        let mut face = self.face_ref();
        // Round-trip: convert normalized coords back to design-space values
        // so ttf-parser can re-normalize them (including avar remapping).
        // Coords are zipped with axes in order — the i-th coord maps to the i-th axis.
        for (axis, coord) in face.0.variation_axes().into_iter().zip(coords.iter()) {
            let value = *coord as f32 / 16384.0;
            let design_value = if value >= 0.0 {
                axis.def_value + value * (axis.max_value - axis.def_value)
            } else {
                axis.def_value + value * (axis.def_value - axis.min_value)
            };
            face.0.set_variation(axis.tag, design_value);
        }
        face
    }

    #[cfg(not(feature = "textlayout"))]
    pub(crate) fn face_ref_with_normalized_coords(&self, _coords: &[i16]) -> FontFaceRef<'_> {
        self.face_ref()
    }

    pub fn metrics(&self, size: f32) -> FontMetrics {
        let mut metrics = self.metrics;

        metrics.scale(self.scale(size));

        metrics
    }

    #[cfg(feature = "textlayout")]
    pub fn metrics_with_variations(&self, size: f32, variations: &FontVariations) -> FontMetrics {
        if variations.is_empty() {
            return self.metrics(size);
        }

        let face = self.face_ref_with_variations(variations);
        let mut metrics = FontMetrics {
            ascender: face.0.ascender() as f32,
            descender: face.0.descender() as f32,
            height: face.0.height() as f32,
            ..self.metrics
        };
        metrics.scale(self.scale(size));
        metrics
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub fn metrics_with_variations(&self, size: f32, variations: &FontVariations) -> FontMetrics {
        if variations.is_empty() {
            return self.metrics(size);
        }
        let Some(font_ref) = self.swash_font_ref() else {
            return self.metrics(size);
        };
        let settings: Vec<swash::Setting<f32>> = variations
            .iter()
            .map(|(tag, value)| swash::Setting {
                tag: swash::tag_from_bytes(&tag.to_be_bytes()),
                value,
            })
            .collect();
        let coords: Vec<swash::NormalizedCoord> = font_ref.variations().normalized_coords(settings).collect();
        let swash_metrics = font_ref.metrics(&coords);
        let mut metrics = FontMetrics {
            ascender: swash_metrics.ascent,
            descender: -swash_metrics.descent,
            height: swash_metrics.ascent + swash_metrics.descent + swash_metrics.leading,
            ..self.metrics
        };
        metrics.scale(self.scale(size));
        metrics
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub fn metrics_with_variations(&self, size: f32, _variations: &FontVariations) -> FontMetrics {
        self.metrics(size)
    }

    pub fn scale(&self, size: f32) -> f32 {
        size / self.units_per_em as f32
    }

    #[cfg(feature = "textlayout")]
    pub fn variation_axes(&self) -> Vec<VariationAxisInfo> {
        let face = self.face_ref();
        let mut axes = Vec::new();
        if let Some(table) = face.0.tables().fvar {
            for axis in table.axes {
                axes.push(VariationAxisInfo {
                    tag: axis.tag.to_bytes(),
                    min_value: axis.min_value,
                    def_value: axis.def_value,
                    max_value: axis.max_value,
                    name_id: axis.name_id,
                    hidden: axis.hidden,
                });
            }
        }
        axes
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub fn variation_axes(&self) -> Vec<VariationAxisInfo> {
        let Some(font_ref) = self.swash_font_ref() else {
            return Vec::new();
        };
        font_ref
            .variations()
            .map(|v| {
                let name_id = match v.name_id() {
                    swash::StringId::Other(id) => id,
                    _ => 0,
                };
                VariationAxisInfo {
                    tag: v.tag().to_be_bytes(),
                    min_value: v.min_value(),
                    def_value: v.default_value(),
                    max_value: v.max_value(),
                    name_id,
                    hidden: v.is_hidden(),
                }
            })
            .collect()
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub fn variation_axes(&self) -> Vec<VariationAxisInfo> {
        Vec::new()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn normalize_variations(&self, variations: &FontVariations) -> Vec<i16> {
        let face = self.face_ref_with_variations(variations);
        face.0.variation_coordinates().iter().map(|c| c.get()).collect()
    }

    #[allow(dead_code)]
    fn hash_normalized_coords(coords: &[i16]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = fnv::FnvHasher::default();
        coords.hash(&mut hasher);
        hasher.finish()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn glyph(
        &self,
        face: &FontFaceRef<'_>,
        codepoint: u16,
        normalized_coords: &[i16],
    ) -> Option<Ref<'_, Glyph>> {
        let cache_key: GlyphCacheKey = (codepoint, Self::hash_normalized_coords(normalized_coords));

        if let Entry::Vacant(entry) = self.glyphs.borrow_mut().entry(cache_key) {
            let mut path = Path::new();

            let id = GlyphId(codepoint);

            let maybe_glyph = if let Some(image) = face
                .0
                .glyph_raster_image(id, u16::MAX)
                .filter(|img| img.format == ttf_parser::RasterImageFormat::PNG)
            {
                let scale = if image.pixels_per_em != 0 {
                    self.units_per_em as f32 / image.pixels_per_em as f32
                } else {
                    1.0
                };
                Some(Glyph {
                    path: None,
                    metrics: GlyphMetrics {
                        width: image.width as f32 * scale,
                        height: image.height as f32 * scale,
                        bearing_x: image.x as f32 * scale,
                        bearing_y: (image.y as f32 + image.height as f32) * scale,
                    },
                })
            } else {
                face.0.outline_glyph(id, &mut path).map(|bbox| Glyph {
                    path: Some(path),
                    metrics: GlyphMetrics {
                        width: bbox.width() as f32,
                        height: bbox.height() as f32,
                        bearing_x: bbox.x_min as f32,
                        bearing_y: bbox.y_max as f32,
                    },
                })
            };

            if let Some(glyph) = maybe_glyph {
                entry.insert(glyph);
            }
        }

        Ref::filter_map(self.glyphs.borrow(), |glyphs| glyphs.get(&cache_key)).ok()
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub(crate) fn glyph(
        &self,
        _face: &FontFaceRef<'_>,
        _codepoint: u16,
        _normalized_coords: &[i16],
    ) -> Option<Ref<'_, Glyph>> {
        None
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub(crate) fn glyph(
        &self,
        _face: &FontFaceRef<'_>,
        codepoint: u16,
        normalized_coords: &[i16],
    ) -> Option<Ref<'_, Glyph>> {
        let cache_key: GlyphCacheKey = (codepoint, Self::hash_normalized_coords(normalized_coords));
        if let Entry::Vacant(entry) = self.glyphs.borrow_mut().entry(cache_key) {
            let font_ref = self.swash_font_ref()?;

            let mut scale_context = self.swash_scale_context().borrow_mut();
            let scaler_builder = scale_context
                .builder(font_ref)
                .size(self.units_per_em as f32)
                .hint(false)
                .normalized_coords(normalized_coords);
            let mut scaler = scaler_builder.build();

            let maybe_glyph = if let Some(outline) = scaler.scale_outline(codepoint) {
                use swash::zeno::{Command, PathData};
                let bounds = outline.bounds();
                let mut path = Path::new();
                for cmd in outline.path().commands() {
                    match cmd {
                        Command::MoveTo(p) => path.move_to(p.x, p.y),
                        Command::LineTo(p) => path.line_to(p.x, p.y),
                        Command::QuadTo(c, p) => path.quad_to(c.x, c.y, p.x, p.y),
                        Command::CurveTo(c1, c2, p) => path.bezier_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y),
                        Command::Close => path.close(),
                    }
                }
                Some(Glyph {
                    path: Some(path),
                    metrics: GlyphMetrics {
                        width: bounds.width(),
                        height: bounds.height(),
                        bearing_x: bounds.min.x,
                        bearing_y: bounds.max.y,
                    },
                })
            } else {
                None
            };

            if let Some(glyph) = maybe_glyph {
                entry.insert(glyph);
            }
        }

        Ref::filter_map(self.glyphs.borrow(), |glyphs| glyphs.get(&cache_key)).ok()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn glyph_rendering_representation(
        &self,
        face: &FontFaceRef<'_>,
        codepoint: u16,
        #[allow(unused_variables)] pixels_per_em: u16,
        normalized_coords: &[i16],
    ) -> Option<GlyphRendering<'_>> {
        #[cfg(feature = "image-loading")]
        if let Some(image) =
            face.0
                .glyph_raster_image(GlyphId(codepoint), pixels_per_em)
                .and_then(|raster_glyph_image| {
                    image::load_from_memory_with_format(raster_glyph_image.data, image::ImageFormat::Png).ok()
                })
        {
            return Some(GlyphRendering::RenderAsImage(image));
        }

        self.glyph(face, codepoint, normalized_coords).and_then(|glyph| {
            Ref::filter_map(glyph, |glyph| glyph.path.as_ref())
                .ok()
                .map(GlyphRendering::RenderAsPath)
        })
    }

    #[cfg(not(feature = "textlayout"))]
    pub(crate) fn glyph_rendering_representation(
        &self,
        _face: &FontFaceRef<'_>,
        codepoint: u16,
        #[allow(unused_variables)] _pixels_per_em: u16,
        normalized_coords: &[i16],
    ) -> Option<GlyphRendering<'_>> {
        self.glyph(_face, codepoint, normalized_coords).and_then(|glyph| {
            Ref::filter_map(glyph, |glyph| glyph.path.as_ref())
                .ok()
                .map(GlyphRendering::RenderAsPath)
        })
    }
}
