
use owned_ttf_parser::{AsFontRef, OwnedFont, Font as TtfFont};

use crate::ErrorKind;

use super::freetype as ft;
use super::fontdb::{
    FontId
};

pub struct Font {
    pub(crate) id: FontId,
    pub(crate) data: Vec<u8>,
    pub(crate) face: ft::Face,
    pub(crate) ttf_font: OwnedFont,
}

impl Font {

    pub fn new(id: FontId, face: ft::Face, data: Vec<u8>) -> Self {
        let ttf_font = OwnedFont::from_vec(data.clone(), 0).unwrap();

        Self {
            id,
            data,
            face,
            ttf_font
        }
    }

    pub fn set_size(&mut self, size: u16) -> Result<(), ErrorKind> {
        self.face.set_pixel_sizes(0, size as u32)?;
        Ok(())
    }

    pub fn postscript_name(&self) -> String {
        self.ttf_font.as_font().post_script_name().unwrap()// TODO: Remove this unwrap
        //self.face.postscript_name().unwrap_or_else(String::new)
    }

    pub fn has_chars(&self, text: &str) -> bool {
        let face = self.ttf_font.as_font();

        text.chars().all(|c| {
            face.glyph_index(c).is_some()
        })
    }

    pub fn font_ref(&self) -> &TtfFont<'_> {
        self.ttf_font.as_font()
    }
}
