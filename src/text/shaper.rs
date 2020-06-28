
use std::str::CharIndices;
use std::hash::{Hash, Hasher};
use std::iter::{Peekable, DoubleEndedIterator};

use unicode_script::{Script, UnicodeScript};
use unicode_bidi::{bidi_class, BidiClass, BidiInfo};

use harfbuzz_rs as hb;

use lru::LruCache;
use fnv::{FnvHasher, FnvBuildHasher};

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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Direction {
    Ltr, Rtl
}

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

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct ShapingId {
    size: u16,
    text_hash: u64,
    weight: Weight,
    width_class: WidthClass,
    font_style: FontStyle,
}

impl ShapingId {
    pub fn new(paint: &Paint, text: &str) -> Self {
        let mut hasher = FnvHasher::default();
        text.hash(&mut hasher);

        ShapingId {
            size: paint.font_size(),
            text_hash: hasher.finish(),
            weight: paint.font_weight,
            width_class: paint.font_width_class,
            font_style: paint.font_style,
        }
    }
}

type Cache<H> = LruCache<ShapingId, Result<TextLayout, ErrorKind>, H>;

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

    pub fn shape(&mut self, x: f32, y: f32, fontdb: &mut FontDb, paint: &Paint, text: &str, max_width: Option<f32>) -> Result<TextLayout, ErrorKind> {

        if false {
            let mut res = Self::shape_internal(fontdb, paint, text)?;
            Self::layout(x, y, fontdb, &mut res, paint)?;

            Ok(res)
        } else {
            let id = ShapingId::new(paint, text);

            if !self.cache.contains(&id) {
                self.cache.put(id, Self::shape_internal(fontdb, paint, text));
            }

            let result = self.cache.get(&id).ok_or(ErrorKind::UnknownError)?;

            if let Ok(layout) = result.as_ref() {
                let mut layout = layout.clone();
                Self::layout(x, y, fontdb, &mut layout, paint)?;
                return Ok(layout);
            }

            Err(ErrorKind::UnknownError)
        }
    }

    fn shape_internal(fontdb: &mut FontDb, paint: &Paint, text: &str) -> Result<TextLayout, ErrorKind> {
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

                // find_font will call the closure with each font matching the provided style
                // until a font capable of shaping the word is found
                let ret = fontdb.find_font(&sub_text, paint, |font| {

                    // Call harfbuzz
                    let output = {
                        // TODO: It may be faster if this is created only once and stored inside the Font struct
                        let hb_font = Self::hb_font(font);
                        let buffer = hb::UnicodeBuffer::new()
                            .add_str(sub_text)
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

                    let mut items = Vec::with_capacity(positions.len());

                    let mut has_missing = false;

                    for (position, (info, c)) in positions.iter().zip(infos.iter().zip(sub_text.chars())) {
                        if info.codepoint == 0 {
                            has_missing = true;
                        }

                        let scale = font.scale(paint.font_size as f32);

                        let start_index = run.start + info.cluster as usize;
                        debug_assert!(text.get(start_index..).is_some());

                        let mut g = ShapedGlyph {
                            c: c,
                            byte_index: start_index,
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

                        items.push(g);
                    }

                    (has_missing, items)
                });

                if let Ok(items) = ret {
                    result.glyphs.extend(items);
                }
            }
        }

        Ok(result)
    }

    // Calculates the x,y coordinates for each glyph based on their advances. Calculates total width and height of the shaped text run
    fn layout(x: f32, y: f32, fontdb: &mut FontDb, res: &mut TextLayout, paint: &Paint) -> Result<(), ErrorKind> {
        let mut cursor_x = x;
        let mut cursor_y = y;

        // Calculate total advance for correct horizontal alignment
        res.width = res.glyphs.iter().fold(0.0, |width, glyph| width + glyph.advance_x + paint.letter_spacing);

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

    fn hb_font(font: &mut Font) -> hb::Owned<hb::Font> {
        let face = hb::Face::new(font.data.clone(), 0);
		hb::Font::new(face)
    }

    fn hb_buffer(text: &str, direction: Direction, script: Script) -> hb::UnicodeBuffer {
        let mut buffer = hb::UnicodeBuffer::new()
            .add_str(text)
            .set_direction(match direction {
                Direction::Ltr => hb::Direction::Ltr,
                Direction::Rtl => hb::Direction::Rtl,
            });

        let script_name = script.short_name();

        if script_name.len() == 4 {
            let script: Vec<char> = script_name.chars().collect();
            buffer = buffer.set_script(hb::Tag::new(script[0], script[1], script[2], script[3]));
        }

        buffer
    }
}

impl From<BidiClass> for Direction {
    fn from(class: BidiClass) -> Self {
        match class {
            BidiClass::L => Direction::Ltr,
            BidiClass::R => Direction::Rtl,
            BidiClass::AL => Direction::Rtl,
            _ => Direction::Ltr
        }
    }
}

pub trait UnicodeScripts<I: Iterator<Item=(usize, char)>> {
    fn unicode_scripts(&self) -> UnicodeScriptIterator<I>;
}

impl<'a> UnicodeScripts<CharIndices<'a>> for &'a str {
    fn unicode_scripts(&self) -> UnicodeScriptIterator<CharIndices<'a>> {
        UnicodeScriptIterator {
            string: self,
            iter: self.char_indices().peekable()
        }
    }
}

pub struct UnicodeScriptIterator<'a, I: Iterator<Item=(usize, char)>> {
    string: &'a str,
    iter: Peekable<I>
}

impl<'a, I: Iterator<Item=(usize, char)>> Iterator for UnicodeScriptIterator<'a, I> {
    type Item = (Script, Direction, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((first_index, first)) = self.iter.next() {
            let direction = Direction::from(bidi_class(first));
            let mut script = first.script();

            let mut last_index = self.string.len();

            while let Some((next_index, next)) = self.iter.peek() {
                let next_script = next.script();

                let next_script = match next_script {
                    Script::Common => script,
                    Script::Inherited => script,
                    _ => next_script
                };

                script = match script {
                    Script::Common => next_script,
                    Script::Inherited => next_script,
                    _ => script
                };

                if next_script == script {
                    self.iter.next();
                } else {
                    last_index = *next_index;
                    break;
                }
            }

            return Some((script, direction, &self.string[first_index..last_index]));
        }

        None
    }
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
        
        if let Some((index, _)) = self.char_indices.find(|(_, c)| c.is_whitespace()) {
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
        
        if let Some((index, _)) = self.char_indices.rfind(|(_, c)| c.is_whitespace()) {
            res = Some(&self.string[index..self.end]);
            self.end = index;
        } else if self.start < self.end {
            res = Some(&self.string[self.start..self.end]);
            self.start = self.end;
        }
        
        res
    }

}