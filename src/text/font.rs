use fnv::FnvHashMap;
use rustybuzz::ttf_parser;
use rustybuzz::ttf_parser::{Face as TtfFont, GlyphId};
use std::cell::{Ref, RefCell};
use std::collections::hash_map::Entry;

use crate::{ErrorKind, Path};

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

pub struct Font {
    data: Box<dyn AsRef<[u8]>>,
    face_index: u32,
    units_per_em: u16,
    metrics: FontMetrics,
    glyphs: RefCell<FnvHashMap<u16, Glyph>>,
}

impl Font {
    pub fn new_with_data<T: AsRef<[u8]> + 'static>(data: T, face_index: u32) -> Result<Self, ErrorKind> {
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

    pub fn face_ref(&self) -> rustybuzz::Face<'_> {
        rustybuzz::Face::from_slice(self.data.as_ref().as_ref(), self.face_index).unwrap()
    }

    pub fn metrics(&self, size: f32) -> FontMetrics {
        let mut metrics = self.metrics;

        metrics.scale(self.scale(size));

        metrics
    }

    pub fn scale(&self, size: f32) -> f32 {
        size / self.units_per_em as f32
    }

    pub fn glyph(&self, face: &rustybuzz::Face<'_>, codepoint: u16) -> Option<Ref<'_, Glyph>> {
        if let Entry::Vacant(entry) = self.glyphs.borrow_mut().entry(codepoint) {
            let mut path = Path::new();

            let id = GlyphId(codepoint);

            let maybe_glyph = if let Some(image) = face
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
                face.outline_glyph(id, &mut path).map(|bbox| Glyph {
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

        Ref::filter_map(self.glyphs.borrow(), |glyphs| glyphs.get(&codepoint)).ok()
    }

    pub fn glyph_rendering_representation(
        &self,
        face: &rustybuzz::Face<'_>,
        codepoint: u16,
        #[allow(unused_variables)] pixels_per_em: u16,
    ) -> Option<GlyphRendering> {
        #[cfg(feature = "image-loading")]
        if let Some(image) = face
            .glyph_raster_image(GlyphId(codepoint), pixels_per_em)
            .and_then(|raster_glyph_image| {
                image::load_from_memory_with_format(raster_glyph_image.data, image::ImageFormat::Png).ok()
            })
        {
            return Some(GlyphRendering::RenderAsImage(image));
        };

        self.glyph(face, codepoint).and_then(|glyph| {
            Ref::filter_map(glyph, |glyph| glyph.path.as_ref())
                .ok()
                .map(GlyphRendering::RenderAsPath)
        })
    }
}
