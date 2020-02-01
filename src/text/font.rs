
use super::freetype as ft;
use super::fontdb::{
    FontId
};

pub struct Font {
    pub(crate) id: FontId,
    pub(crate) face: ft::Face,
}

impl Font {

    pub fn new(id: FontId, face: ft::Face) -> Self {
        Self {
            id,
            face
        }
    }

    pub fn set_size(&mut self, size: u16) {
        self.face.set_pixel_sizes(0, size as u32).unwrap();
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
