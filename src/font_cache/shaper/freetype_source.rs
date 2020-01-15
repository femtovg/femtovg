
use freetype as ft;

use super::ShaperSource;

impl ShaperSource for ft::Face {
    fn glyph_index(&self, unicode: char) -> Option<u32> {
        let index = self.get_char_index(unicode as usize);

        if index != 0 {
            Some(index)
        } else {
            None
        }
    }

    fn glyph_variation(&self, _unicode: char, _variation_sel: char) -> Option<u32> {
        // TODO
        None
    }

    fn glyph_advance(&self, glyph_index: u32) -> (i32, i32) {
        let _ = self.load_glyph(glyph_index, ft::face::LoadFlag::DEFAULT);

        let adv = self.glyph().advance();

        (adv.x as i32 >> 6, adv.y as i32 >> 6)
    }

    fn glyph_bearing(&self, glyph_index: u32) -> (i32, i32) {
        let _ = self.load_glyph(glyph_index, ft::face::LoadFlag::DEFAULT);

        (self.glyph().bitmap_left(), self.glyph().bitmap_top())
    }

    fn glyph_size(&self, glyph_index: u32) -> (i32, i32) {
        let _ = self.load_glyph(glyph_index, ft::face::LoadFlag::DEFAULT);

        let metrics = self.glyph().metrics();

        (metrics.width as i32, metrics.height as i32)
    }

    fn glyphs_kerning(&self, left_glyph_index: u32, right_glyph_index: u32) -> Option<(i32, i32)> {
        let kern = self.get_kerning(left_glyph_index, right_glyph_index, ft::face::KerningMode::KerningDefault).ok();

        if let Some(kern) = kern {
            Some((kern.x as i32 >> 6, kern.y as i32 >> 6))
        } else {
            None
        }
    }
}
