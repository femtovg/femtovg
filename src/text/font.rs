use fnv::FnvHashMap;
use owned_ttf_parser::{
    AsFontRef,
    Font as TtfFont,
    GlyphId,
    OwnedFont,
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
    pub path: Path,
    pub metrics: GlyphMetrics,
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
    data: Vec<u8>,
    owned_ttf_font: OwnedFont,
    units_per_em: u16,
    metrics: FontMetrics,
    glyphs: FnvHashMap<u16, Glyph>,
}

impl Font {
    pub fn new(data: &[u8]) -> Result<Self, ErrorKind> {
        let owned_ttf_font = OwnedFont::from_vec(data.to_owned(), 0).ok_or(ErrorKind::FontParseError)?;

        let units_per_em = owned_ttf_font
            .as_font()
            .units_per_em()
            .ok_or(ErrorKind::FontInfoExtracionError)?;

        let ttf_font = owned_ttf_font.as_font();

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
            data: data.to_owned(),
            owned_ttf_font,
            units_per_em,
            metrics,
            glyphs: Default::default(),
        })
    }

    pub fn data(&self) -> &[u8] {
        self.data.as_ref()
    }

    fn font_ref(&self) -> &TtfFont<'_> {
        self.owned_ttf_font.as_font()
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

            if let Some(bbox) = self.font_ref().outline_glyph(id, &mut path) {
                self.glyphs.insert(
                    codepoint,
                    Glyph {
                        path,
                        metrics: GlyphMetrics {
                            width: bbox.width() as f32,
                            height: bbox.height() as f32,
                            bearing_x: bbox.x_min as f32,
                            bearing_y: bbox.y_max as f32,
                        },
                    },
                );
            }
        }

        self.glyphs.get_mut(&codepoint)
    }
}
