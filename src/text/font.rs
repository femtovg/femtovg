
use crate::ErrorKind;

use super::freetype as ft;
use super::fontdb::{
    FontId
};

pub struct Font {
    pub(crate) id: FontId,
    pub(crate) face: ft::Face,
    pub(crate) data: Vec<u8>
}

impl Font {

    pub fn new(id: FontId, face: ft::Face, data: Vec<u8>) -> Self {
        Self {
            id,
            face,
            data
        }
    }

    pub fn set_size(&mut self, size: u16) -> Result<(), ErrorKind> {
        self.face.set_pixel_sizes(0, size as u32)?;
        Ok(())
    }

    pub fn postscript_name(&self) -> String {
        self.face.postscript_name().unwrap_or_else(String::new)
    }

    pub fn has_chars(&self, text: &str) -> bool {
        text.chars().all(|c| {
            self.face.get_char_index(c as u32) != 0
        })
    }

}
