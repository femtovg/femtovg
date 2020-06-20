
use fnv::FnvHashMap;
use owned_ttf_parser::{AsFontRef, OwnedFont, Font as TtfFont, GlyphId};

use crate::ErrorKind;

use super::fontdb::{
    FontId
};

pub struct Metrics {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) bearing_x: f32,
    pub(crate) bearing_y: f32
}

pub struct Font {
    pub(crate) id: FontId,
    pub(crate) data: Vec<u8>,
    pub(crate) owned_ttf_font: OwnedFont,
    pub(crate) units_per_em: u16,
    glyphs: FnvHashMap<char, Metrics>
}

impl Font {

    pub fn new(id: FontId, data: Vec<u8>) -> Result<Self, ErrorKind> {
        let owned_ttf_font = OwnedFont::from_vec(data.clone(), 0).unwrap();

        let units_per_em = owned_ttf_font.as_font().units_per_em().ok_or(ErrorKind::FontInfoExtracionError)?;

        Ok(Self {
            id,
            data,
            owned_ttf_font,
            units_per_em,
            glyphs: Default::default()
        })
    }

    pub fn postscript_name(&self) -> String {
        self.owned_ttf_font.as_font().post_script_name().unwrap()// TODO: Remove this unwrap
        //self.face.postscript_name().unwrap_or_else(String::new)
    }

    pub fn has_chars(&self, text: &str) -> bool {
        let face = self.owned_ttf_font.as_font();

        text.chars().all(|c| {
            face.glyph_index(c).is_some()
        })
    }

    pub fn font_ref(&self) -> &TtfFont<'_> {
        self.owned_ttf_font.as_font()
    }

    pub fn ascender(&self, size: f32) -> f32 {
        self.font_ref().ascender() as f32 * self.scale(size)
    }

    pub fn descender(&self, size: f32) -> f32 {
        self.font_ref().descender() as f32 * self.scale(size)
    }

    pub fn height(&self, size: f32) -> f32 {
        self.font_ref().height() as f32 * self.scale(size)
    }

    pub fn scale(&self, size: f32) -> f32 {
        size / self.units_per_em as f32
    }

    // pub fn glyph_metrics(&mut self, c: char, size: f32) -> &Metrics {
    //     self.glyphs.entry(c).or_insert_with(|| {

    //     })
    // }
}
