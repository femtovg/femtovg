use fnv::FnvHashMap;
use owned_ttf_parser::{AsFontRef, Font as TtfFont, GlyphId, OwnedFont};

use crate::{
    ErrorKind, Path,
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

pub struct FontMetrics {
    ascender: f32,
    descender: f32,
    height: f32,
}

impl FontMetrics {
    fn scale(&mut self, scale: f32) {
        self.ascender *= scale;
        self.descender *= scale;
        self.height *= scale;
    }

    pub fn ascender(&self) -> f32 {
        self.ascender
    }

    pub fn descender(&self) -> f32 {
        self.descender
    }

    pub fn height(&self) -> f32 {
        self.height
    }
}

pub(crate) struct Font {
    data: Vec<u8>,
    owned_ttf_font: OwnedFont,
    units_per_em: u16,
    glyphs: FnvHashMap<u16, Glyph>,
}

impl Font {
    pub fn new(data: &[u8]) -> Result<Self, ErrorKind> {
        let owned_ttf_font = OwnedFont::from_vec(data.to_owned(), 0).ok_or(ErrorKind::FontParseError)?;

        let units_per_em = owned_ttf_font
            .as_font()
            .units_per_em()
            .ok_or(ErrorKind::FontInfoExtracionError)?;

        Ok(Self {
            data: data.to_owned(),
            owned_ttf_font,
            units_per_em,
            glyphs: Default::default(),
        })
    }

    pub fn data(&self) -> &[u8] {
        self.data.as_ref()
    }

    // pub fn postscript_name(&self) -> String {
    //     self.owned_ttf_font.as_font().post_script_name().unwrap()// TODO: Remove this unwrap
    //     //self.face.postscript_name().unwrap_or_else(String::new)
    // }

    fn font_ref(&self) -> &TtfFont<'_> {
        self.owned_ttf_font.as_font()
    }

    pub fn metrics(&self, size: f32) -> FontMetrics {
        let mut metrics = FontMetrics {
            ascender: self.font_ref().ascender() as f32,
            descender: self.font_ref().descender() as f32,
            height: self.font_ref().height() as f32,
        };

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