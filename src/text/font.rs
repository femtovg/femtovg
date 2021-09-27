use fnv::FnvHashMap;
use ttf_parser::{
    Face as TtfFont,
    GlyphId,
};

use crate::{
    ErrorKind,
    Path,
};

pub struct GlyphMetrics {
    pub width: f32,
    pub height: f32,
    pub bearing_x: f32,
    pub bearing_y: f32,
}

pub struct Glyph {
    pub path: Option<Path>, // None means render as image
    pub metrics: GlyphMetrics,
}

pub(crate) enum GlyphRendering<'a> {
    RenderAsPath(&'a mut Path),
    #[cfg(feature = "image-loading")]
    RenderAsImage(image::DynamicImage),
}

/// Information about a font.
// TODO: underline, strikeout, subscript, superscript metrics
#[derive(Copy, Clone, Default, Debug)]
pub struct FontMetrics {
    ascender: f32,
    descender: f32,
    height: f32,
    regular: bool,
    italic: bool,
    bold: bool,
    oblique: bool,
    variable: bool,
    weight: u16,
    width: u16,
}

impl FontMetrics {
    fn scale(&mut self, scale: f32) {
        self.ascender *= scale;
        self.descender *= scale;
        self.height *= scale;
    }

    /// The distance from the baseline to the top of the highest glyph
    pub fn ascender(&self) -> f32 {
        self.ascender
    }

    /// The distance from the baseline to the bottom of the lowest descenders on the glyphs
    pub fn descender(&self) -> f32 {
        self.descender
    }

    pub fn height(&self) -> f32 {
        self.height.round()
    }

    pub fn regular(&self) -> bool {
        self.regular
    }

    pub fn italic(&self) -> bool {
        self.italic
    }

    pub fn bold(&self) -> bool {
        self.bold
    }

    pub fn oblique(&self) -> bool {
        self.oblique
    }

    pub fn variable(&self) -> bool {
        self.variable
    }

    pub fn weight(&self) -> u16 {
        self.weight
    }

    pub fn width(&self) -> u16 {
        self.width
    }
}

pub(crate) struct Font {
    data: Box<dyn AsRef<[u8]>>,
    face_index: u32,
    units_per_em: u16,
    metrics: FontMetrics,
    glyphs: FnvHashMap<u16, Glyph>,
}

impl Font {
    pub fn new<T: AsRef<[u8]> + 'static>(data: T, face_index: u32) -> Result<Self, ErrorKind> {
        let ttf_font =
            TtfFont::from_slice(data.as_ref().as_ref(), face_index).map_err(|_| ErrorKind::FontParseError)?;

        let units_per_em = ttf_font.units_per_em().ok_or(ErrorKind::FontInfoExtracionError)?;

        let metrics = FontMetrics {
            ascender: ttf_font.ascender() as f32,
            descender: ttf_font.descender() as f32,
            height: ttf_font.height() as f32,
            regular: ttf_font.is_regular(),
            italic: ttf_font.is_italic(),
            bold: ttf_font.is_bold(),
            oblique: ttf_font.is_oblique(),
            variable: ttf_font.is_variable(),
            weight: ttf_font.width().to_number(),
            width: ttf_font.weight().to_number(),
        };

        Ok(Self {
            data: Box::new(data),
            face_index,
            units_per_em,
            metrics,
            glyphs: Default::default(),
        })
    }

    pub fn data(&self) -> &[u8] {
        self.data.as_ref().as_ref()
    }

    fn font_ref(&self) -> TtfFont<'_> {
        TtfFont::from_slice(self.data.as_ref().as_ref(), self.face_index).unwrap()
    }

    pub fn metrics(&self, size: f32) -> FontMetrics {
        let mut metrics = self.metrics;

        metrics.scale(self.scale(size));

        metrics
    }

    pub fn scale(&self, size: f32) -> f32 {
        size / self.units_per_em as f32
    }

    pub fn glyph(&mut self, codepoint: u16) -> Option<&mut Glyph> {
        if !self.glyphs.contains_key(&codepoint) {
            let mut path = Path::new();

            let id = GlyphId(codepoint);

            let maybe_glyph = if let Some(image) = self
                .font_ref()
                .glyph_raster_image(id, std::u16::MAX)
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
            } else if let Some(bbox) = self.font_ref().outline_glyph(id, &mut path) {
                Some(Glyph {
                    path: Some(path),
                    metrics: GlyphMetrics {
                        width: bbox.width() as f32,
                        height: bbox.height() as f32,
                        bearing_x: bbox.x_min as f32,
                        bearing_y: bbox.y_max as f32,
                    },
                })
            } else {
                None
            };

            if let Some(glyph) = maybe_glyph {
                self.glyphs.insert(codepoint, glyph);
            }
        }

        self.glyphs.get_mut(&codepoint)
    }

    pub fn glyph_rendering_representation(&mut self, codepoint: u16, _pixels_per_em: u16) -> Option<GlyphRendering> {
        #[cfg(feature = "image-loading")]
        if let Some(image) = self
            .font_ref()
            .glyph_raster_image(GlyphId(codepoint), _pixels_per_em)
            .and_then(|raster_glyph_image| {
                image::load_from_memory_with_format(raster_glyph_image.data, image::ImageFormat::Png).ok()
            })
        {
            return Some(GlyphRendering::RenderAsImage(image));
        };

        self.glyph(codepoint)
            .and_then(|glyph| glyph.path.as_mut().map(|path| GlyphRendering::RenderAsPath(path)))
    }
}
