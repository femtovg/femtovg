
use std::str::CharIndices;
use std::hash::{Hash, Hasher};
use std::iter::DoubleEndedIterator;

use lru::LruCache;
use fnv::{FnvHasher, FnvBuildHasher};

use harfbuzz_rs as hb;

use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Paint,
    ErrorKind
};

use super::{
    Align,
    Baseline,
    Weight,
    WidthClass,
    FontStyle,
    Font,
    FontDb,
    FontId,
    TextLayout
};

const LRU_CACHE_CAPACITY: usize = 1000;

#[derive(Copy, Clone, Debug, Default)]
pub struct ShapedGlyph {
    pub x: f32,
    pub y: f32,
    pub c: char,
    pub byte_index: usize,
    pub font_id: FontId,
    pub codepoint: u32,
    pub width: f32,
    pub height: f32,
    pub advance_x: f32,
    pub advance_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub bearing_x: f32,
    pub bearing_y: f32,
}

#[derive(Clone, Debug, Default)]
struct ShapedWord {
    glyphs: Vec<ShapedGlyph>,
    width: f32,
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct ShapingId {
    size: u16,
    word_hash: u64,
    weight: Weight,
    width_class: WidthClass,
    font_style: FontStyle
}

impl ShapingId {
    pub fn new(paint: &Paint, word: &str) -> Self {
        let mut hasher = FnvHasher::default();
        word.hash(&mut hasher);

        ShapingId {
            size: paint.font_size(),
            word_hash: hasher.finish(),
            weight: paint.font_weight,
            width_class: paint.font_width_class,
            font_style: paint.font_style,
        }
    }
}

type Cache<H> = LruCache<ShapingId, Result<ShapedWord, ErrorKind>, H>;

pub struct Shaper {
    cache: Cache<FnvBuildHasher>,
}

impl Default for Shaper {
    fn default() -> Self {
        let fnv = FnvBuildHasher::default();

        Self {
            cache: LruCache::with_hasher(LRU_CACHE_CAPACITY, fnv)
        }
    }
}

impl Shaper {
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn shape(&mut self, x: f32, y: f32, fontdb: &mut FontDb, paint: &Paint, text: &str, max_width: Option<u32>) -> Result<TextLayout, ErrorKind> {
        let mut result = TextLayout {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            glyphs: Vec::with_capacity(text.len())
        };

        let bidi_info = BidiInfo::new(&text, None);

        for paragraph in &bidi_info.paragraphs {
            let line = paragraph.range.clone();

            let (levels, runs) = bidi_info.visual_runs(&paragraph, line);
            
            for run in runs.iter() {
                let sub_text = &text[run.clone()];

                if sub_text.is_empty() {
                    continue;
                }

                let hb_direction = if levels[run.start].is_rtl() {
                    hb::Direction::Rtl
                } else {
                    hb::Direction::Ltr
                };

                let mut words = Vec::new();
                let mut word_break_reached = false;

                for word in sub_text.split_word_bounds() {
                    let id = ShapingId::new(paint, word);

                    if !self.cache.contains(&id) {
                        let word = Self::shape_word(word, hb_direction, fontdb, paint);
                        self.cache.put(id, word);
                    }

                    if let Some(Ok(word)) = self.cache.get(&id) {
                        if let Some(max_width) = max_width {
                            if result.width + word.width > max_width as f32 {
                                word_break_reached = true;
                                break;
                            }
                        }
                        
                        result.width += word.width;
                        words.push(word.clone());
                    }
                }

                if levels[run.start].is_rtl() {
                    words.reverse();
                }

                for word in words {
                    result.glyphs.extend(word.glyphs.clone());
                }

                if word_break_reached {
                    break;
                }
            }
        }

        Self::layout(x, y, fontdb, &mut result, paint)?;

        Ok(result)
    }

    fn shape_word(word: &str, hb_direction: hb::Direction, fontdb: &mut FontDb, paint: &Paint) -> Result<ShapedWord, ErrorKind> {
        // find_font will call the closure with each font matching the provided style
        // until a font capable of shaping the word is found
        let ret = fontdb.find_font(&word, paint, |font| {

            // Call harfbuzz
            let output = {
                // TODO: It may be faster if this is created only once and stored inside the Font struct
                let face = hb::Face::new(font.data.clone(), 0);
                let hb_font = hb::Font::new(face);
                
                let buffer = hb::UnicodeBuffer::new()
                    .add_str(word)
                    .set_direction(hb_direction);

                hb::shape(&hb_font, buffer, &[])
            };

            // let output = {
            //     let rb_font = Self::rb_font(font);
            //     //rb_font.set_scale(style.size, style.size);
            //     let buffer = Self::rb_buffer(&word, direction, script);
            //
            //     rustybuzz::shape(&rb_font, buffer, &[])
            // };

            let positions = output.get_glyph_positions();
            let infos = output.get_glyph_infos();

            let mut shaped_word = ShapedWord {
                glyphs: Vec::with_capacity(positions.len()),
                width: 0.0
            };

            let mut has_missing = false;

            for (position, (info, c)) in positions.iter().zip(infos.iter().zip(word.chars())) {
                if info.codepoint == 0 {
                    has_missing = true;
                }

                let scale = font.scale(paint.font_size as f32);

                //let start_index = run.start + info.cluster as usize;
                //debug_assert!(text.get(start_index..).is_some());

                let mut g = ShapedGlyph {
                    c: c,
                    byte_index: 0, // TODO
                    font_id: font.id,
                    codepoint: info.codepoint,
                    advance_x: position.x_advance as f32 * scale,
                    advance_y: position.y_advance as f32 * scale,
                    offset_x: position.x_offset as f32 * scale,
                    offset_y: position.y_offset as f32 * scale,
                    ..Default::default()
                };

                if let Some(glyph) = font.glyph(info.codepoint as u16) {
                    g.width = glyph.metrics.width * scale;
                    g.height = glyph.metrics.height * scale;
                    g.bearing_x = glyph.metrics.bearing_x * scale;
                    g.bearing_y = glyph.metrics.bearing_y * scale;
                }

                shaped_word.width += g.advance_x + paint.letter_spacing;
                shaped_word.glyphs.push(g);
            }

            (has_missing, shaped_word)
        });

        ret
    }

    // Calculates the x,y coordinates for each glyph based on their advances. Calculates total width and height of the shaped text run
    fn layout(x: f32, y: f32, fontdb: &mut FontDb, res: &mut TextLayout, paint: &Paint) -> Result<(), ErrorKind> {
        let mut cursor_x = x;
        let mut cursor_y = y;

        // Horizontal alignment
        match paint.text_align {
            Align::Center => cursor_x -= res.width / 2.0,
            Align::Right => cursor_x -= res.width,
            _ => ()
        }

        res.x = cursor_x;

        let mut height = 0.0f32;
        let mut y = cursor_y;

        for glyph in &mut res.glyphs {
            let font = fontdb.get_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
            
            // Baseline alignment
            let ascender = font.ascender(paint.font_size as f32);
            let descender = font.descender(paint.font_size as f32);

            let alignment_offset_y = match paint.text_baseline {
                Baseline::Top => ascender,
                Baseline::Middle => (ascender + descender) / 2.0,
                Baseline::Alphabetic => 0.0,
                Baseline::Bottom => descender,
            };

            glyph.x = cursor_x + glyph.offset_x + glyph.bearing_x;
            glyph.y = cursor_y + glyph.offset_y - glyph.bearing_y + alignment_offset_y;

            height = height.max(font.height(paint.font_size as f32));
            y = y.min(glyph.y);

            cursor_x += glyph.advance_x + paint.letter_spacing;
            cursor_y += glyph.advance_y;
        }

        res.y = y;
        res.height = height;

        Ok(())
    }

    // TODO: error handling
    // fn rb_font(font: &mut Font) -> rustybuzz::Font {
    //     let face = match rustybuzz::Face::new(&font.data, 0) {
    //         Some(v) => v,
    //         None => {
    //             eprintln!("Error: malformed font.");
    //             std::process::exit(1);
    //         }
    //     };
    //
    //     rustybuzz::Font::new(face)
    // }
    //
    // fn rb_buffer(text: &str, direction: Direction, script: Script) -> rustybuzz::Buffer {
    //     let mut buffer = rustybuzz::Buffer::new(text);
    //
    //     // TODO: Direction and script
    //
    //     buffer
    // }
}

trait SplitWhitespaceInclusive {
    fn split_whitespace_inclusive(&self) -> SplitWhitespaceInclusiveIter;
}

impl SplitWhitespaceInclusive for &str {
    fn split_whitespace_inclusive(&self) -> SplitWhitespaceInclusiveIter {
        SplitWhitespaceInclusiveIter {
            start: 0,
            end: self.len(),
            string: self,
            char_indices: self.char_indices()
        }
    }
}

struct SplitWhitespaceInclusiveIter<'a> {
    start: usize,
    end: usize,
    string: &'a str,
    char_indices: CharIndices<'a>
}

impl<'a> Iterator for SplitWhitespaceInclusiveIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let mut res = None;

        if let Some((index, _)) = self.char_indices.find(|(_, c)| c.is_ascii_whitespace()) {
            res = Some(&self.string[self.start..index]);
            self.start = index;
        } else if self.start < self.end {
            res = Some(&self.string[self.start..self.end]);
            self.start = self.end;
        }

        res
    }
}

impl<'a> DoubleEndedIterator for SplitWhitespaceInclusiveIter<'a> {

    fn next_back(&mut self) -> Option<Self::Item> {
        let mut res = None;

        if let Some((index, _)) = self.char_indices.rfind(|(_, c)| c.is_ascii_whitespace()) {
            res = Some(&self.string[index..self.end]);
            self.end = index;
        } else if self.start < self.end {
            res = Some(&self.string[self.start..self.end]);
            self.start = self.end;
        }

        res
    }

}