
use std::fmt;
use std::error::Error;

use unicode_normalization::{char, UnicodeNormalization};

mod freetype_source;

pub trait ShaperSource {
    fn glyph_index(&self, unicode: char) -> Option<u32>;
    fn glyph_variation(&self, unicode: char, variation_sel: char) -> Option<u32>;
    fn glyph_advance(&self, glyph_index: u32) -> (i32, i32);
    fn glyph_bearing(&self, glyph_index: u32) -> (i32, i32);
    fn glyph_size(&self, glyph_index: u32) -> (i32, i32);
    fn glyphs_kerning(&self, left_glyph_index: u32, right_glyph_index: u32) -> Option<(i32, i32)>;
}

type Result<T> = std::result::Result<T, ShaperError>;

pub struct GlyphInfo {
    pub glyph_index: u32,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32
}


pub fn shape(source: &dyn ShaperSource, text: &str) -> Result<Vec<GlyphInfo>> {
    let mut positions: Vec<GlyphInfo> = Vec::new();

    // convert to glyph ids
    let ids = find_glyph_indexes(source, text);

    let mut ids_iter = ids.into_iter().peekable();

    while let Some((_c, glyph_index)) = ids_iter.next() {
        let mut info = GlyphInfo {
            glyph_index: glyph_index,
            x_advance: 0,
            y_advance: 0,
            x_offset: 0,
            y_offset: 0,
        };

        let mut advance = source.glyph_advance(glyph_index);

        // TTF Kerning
        if let Some((_, next_index)) = ids_iter.peek() {
            if let Some(kerning) = source.glyphs_kerning(glyph_index, *next_index) {
                advance.0 += kerning.0;
                advance.1 += kerning.1;
            }
        }

        info.x_advance = advance.0;
        info.y_advance = advance.1;

        positions.push(info);
    }

    Ok(positions)
}

fn find_glyph_indexes(source: &dyn ShaperSource, text: &str) -> Vec<(char, u32)> {
    let mut ids = Vec::new();

    // Iterate over the composed form of characters, the composed form
    // will prpbably be better designed if it's present in the font
    let mut chars = text.chars().nfc().peekable();

    while let Some(c) = chars.next() {
        // check for variation with the next char
        if let Some(next_char) = chars.peek() {
            if let Some(variation_id) = source.glyph_variation(c, *next_char) {
                ids.push((c, variation_id));
                chars.next();
                continue;
            }
        }

        if let Some(glyph_id) = source.glyph_index(c) {
            ids.push((c, glyph_id));
        } else if !char::is_combining_mark(c) {
            let mut decomposed = Vec::new(); // TODO: maybe use smallvec

            // try to find glyph ids from the decomposed char
            char::decompose_canonical(c, |c| {
                decomposed.push((c, source.glyph_index(c).unwrap_or(0)));
            });

            // if all scalars are not present in the font file show just a single glyph id 0
            if decomposed.iter().all(|tuple| tuple.1 == 0) {
                ids.push((c, 0));
            } else {
                ids.append(&mut decomposed);
            }
        }
    }

    ids
}


#[derive(Debug)]
pub enum ShaperError {
    GeneralError(String)
}

impl fmt::Display for ShaperError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "canvas error")
    }
}

impl From<&str> for ShaperError {
    fn from(error: &str) -> Self {
        Self::GeneralError(error.to_string())
    }
}

impl Error for ShaperError {}
