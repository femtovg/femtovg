
use std::fmt;
use std::error::Error;

use ttf_parser::{GlyphId, Font};
use unicode_normalization::{char, UnicodeNormalization};

mod combining_class;
use combining_class::CombiningClass;

type Result<T> = std::result::Result<T, ShaperError>;

pub struct GlyphInfo {
    pub glyph_index: u32,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32
}

pub struct ShaperFace<'a> {
    face: Font<'a>,
    units_per_em: f32,
}

impl<'a> ShaperFace<'a> {

    pub fn new(data: &'a [u8]) -> Result<Self> {
        let face = Font::from_data(&data, 0)?;
        let units_per_em = face.units_per_em().ok_or("invalid units per em")?;

        Ok(Self {
            face: face,
            units_per_em: units_per_em as f32
        })
    }

    pub fn shape(&self, text: &str, size: u32) -> Result<Vec<GlyphInfo>> {
        let mut positions: Vec<GlyphInfo> = Vec::new();

        let scale = size as f32 / self.units_per_em;

        // convert to glyph ids
        let ids = self.find_glyph_ids(text);

        let mut ids_iter = ids.into_iter().peekable();

        let mut prev = None;

        while let Some((c, glyph_id)) = ids_iter.next() {
            let mut info = GlyphInfo {
                glyph_index: glyph_id.0 as u32,
                x_advance: 0,
                y_advance: 0,
                x_offset: 0,
                y_offset: 0,
            };

            let hmetrics = self.face.glyph_hor_metrics(glyph_id)?;

            let mut x_advance = hmetrics.advance as i16;

            // TTF Kerning
            if let Some((_, next_id)) = ids_iter.peek() {
                if let Ok(kerning) = self.face.glyphs_kerning(glyph_id, *next_id) {
                    x_advance += kerning;
                }
            }

            info.x_advance = (x_advance as f32 * scale) as i32;

            if char::is_combining_mark(c) {
                if let Some(class) = CombiningClass::new(char::canonical_combining_class(c)) {

                    if let Some((prev_c, prev_glyph_id)) = prev {
                        let prev_info = positions.last_mut().unwrap();

                        info.x_advance = prev_info.x_advance;
                        prev_info.x_advance = 0;// TODO: place in middle

                        info.y_offset = -(size as i32);
                    }

                }
            }

            positions.push(info);
            prev = Some((c, glyph_id));
        }

        Ok(positions)
    }

    fn find_glyph_ids(&self, text: &str) -> Vec<(char, GlyphId)> {
        let mut ids = Vec::new();

        for c in text.chars().nfc() {

            if let Ok(glyph_id) = self.face.glyph_index(c) {
                ids.push((c, glyph_id)); // present in font
            } else {
                // try to find glyph id from the decomposed char
                char::decompose_canonical(c, |c| {
                    if let Ok(glyph_id) = self.face.glyph_index(c) {
                        ids.push((c, glyph_id)); // present in font
                    } else {
                        ids.push((c, GlyphId(0)));
                    }
                });
            }

            //ids.push(current_char_id);
        }

        // check for variation with the next char
        /*
        if let Some(next_char) = chars.peek() {
            if let Ok(variation_id) = self.face.glyph_variation_index(c, *next_char) {
                ids.push(variation_id);
                chars.next();
                continue;
            }
        }*/

        ids
    }

}

#[derive(Debug)]
pub enum ShaperError {
    GeneralError(String),
    TtfParserError(ttf_parser::Error)
}

impl fmt::Display for ShaperError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "canvas error")
    }
}

impl From<ttf_parser::Error> for ShaperError {
    fn from(error: ttf_parser::Error) -> Self {
        Self::TtfParserError(error)
    }
}

impl From<&str> for ShaperError {
    fn from(error: &str) -> Self {
        Self::GeneralError(error.to_string())
    }
}

impl Error for ShaperError {}
