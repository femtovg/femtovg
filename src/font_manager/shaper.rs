
use std::fmt;
use std::error::Error;

use ttf_parser::{GlyphId, Font};
use unicode_normalization::{char, UnicodeNormalization};

// TODO: Lose the dependency on ttf parser and make a trait ShaperInfo
// them implement ShaperInfo fot freetype face

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

        while let Some((_c, glyph_id)) = ids_iter.next() {
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

            positions.push(info);
        }

        Ok(positions)
    }

    fn find_glyph_ids(&self, text: &str) -> Vec<(char, GlyphId)> {
        let mut ids = Vec::new();

        // Iterate over the composed form of characters, the composed form
        // will prpbably be better designed if it's present in the font
        let mut chars = text.chars().nfc().peekable();

        while let Some(c) = chars.next() {
            // check for variation with the next char
            if let Some(next_char) = chars.peek() {
                if let Ok(variation_id) = self.face.glyph_variation_index(c, *next_char) {
                    ids.push((c, variation_id));
                    chars.next();
                    continue;
                }
            }

            if let Ok(glyph_id) = self.face.glyph_index(c) {
                ids.push((c, glyph_id));
            } else if !char::is_combining_mark(c) {
                let mut decomposed = Vec::new(); // TODO: maybe use smallvec

                // try to find glyph ids from the decomposed char
                char::decompose_canonical(c, |c| {
                    decomposed.push((c, self.face.glyph_index(c).unwrap_or(GlyphId(0))));
                });

                // if all scalars are not present in the font file show just a single glyph id 0
                if decomposed.iter().all(|tuple| tuple.1 == GlyphId(0)) {
                    ids.push((c, GlyphId(0)));
                } else {
                    ids.append(&mut decomposed);
                }
            }
        }

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
