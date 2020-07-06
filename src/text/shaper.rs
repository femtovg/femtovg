use std::hash::{Hash, Hasher};

use fnv::{FnvBuildHasher, FnvHasher};
use lru::LruCache;

use harfbuzz_rs as hb;

use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

use crate::{ErrorKind, Paint};

use super::{Align, Baseline, FontDb, FontId, TextMetrics};

const LRU_CACHE_CAPACITY: usize = 1000;

#[derive(Copy, Clone, Debug)]
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
    size: u32,
    word_hash: u64,
    font_ids: [Option<FontId>; 8],
}

impl ShapingId {
    pub fn new(paint: &Paint, word: &str) -> Self {
        let mut hasher = FnvHasher::default();
        word.hash(&mut hasher);

        ShapingId {
            size: (paint.font_size * 10.0).trunc() as u32,
            word_hash: hasher.finish(),
            font_ids: paint.font_ids,
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
            cache: LruCache::with_hasher(LRU_CACHE_CAPACITY, fnv),
        }
    }
}

impl Shaper {
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn shape(
        &mut self,
        x: f32,
        y: f32,
        fontdb: &mut FontDb,
        paint: &Paint,
        text: &str,
        max_width: Option<f32>,
    ) -> Result<TextMetrics, ErrorKind> {
        let mut result = TextMetrics {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            glyphs: Vec::with_capacity(text.len()),
            final_byte_index: 0,
        };

        let bidi_info = BidiInfo::new(&text, Some(unicode_bidi::Level::ltr()));

        if let Some(paragraph) = bidi_info.paragraphs.get(0) {
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
                let mut byte_index = run.start;

                for word in sub_text.split_word_bounds() {
                    let id = ShapingId::new(paint, word);

                    if !self.cache.contains(&id) {
                        let word = Self::shape_word(word, hb_direction, fontdb, paint);
                        self.cache.put(id, word);
                    }

                    if let Some(Ok(word)) = self.cache.get(&id) {
                        let mut word = word.clone();

                        if let Some(max_width) = max_width {
                            if result.width + word.width >= max_width {
                                word_break_reached = true;
                                break;
                            }
                        }

                        result.width += word.width;

                        for glyph in &mut word.glyphs {
                            glyph.byte_index += byte_index;
                            debug_assert!(text.get(glyph.byte_index..).is_some());
                        }

                        words.push(word);
                    }

                    byte_index += word.len();
                }

                if levels[run.start].is_rtl() {
                    words.reverse();
                }

                for word in words {
                    result.glyphs.extend(word.glyphs.clone());
                }

                result.final_byte_index = byte_index;

                if word_break_reached {
                    break;
                }
            }
        }

        Self::layout(x, y, fontdb, &mut result, paint)?;

        Ok(result)
    }

    fn shape_word(
        word: &str,
        hb_direction: hb::Direction,
        fontdb: &mut FontDb,
        paint: &Paint,
    ) -> Result<ShapedWord, ErrorKind> {
        // find_font will call the closure with each font matching the provided style
        // until a font capable of shaping the word is found
        fontdb.find_font(&word, paint, |(font_id, font)| {
            // Call harfbuzz
            let output = {
                // TODO: It may be faster if this is created only once and stored inside the Font struct
                let face = hb::Face::new(font.data.clone(), 0);
                let hb_font = hb::Font::new(face);

                let buffer = hb::UnicodeBuffer::new().add_str(word).set_direction(hb_direction);

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
                width: 0.0,
            };

            let mut has_missing = false;

            for (position, (info, c)) in positions.iter().zip(infos.iter().zip(word.chars())) {
                if info.codepoint == 0 {
                    has_missing = true;
                }

                let scale = font.scale(paint.font_size);

                //let start_index = run.start + info.cluster as usize;
                //debug_assert!(text.get(start_index..).is_some());

                let mut g = ShapedGlyph {
                    x: 0.0,
                    y: 0.0,
                    c: c,
                    byte_index: info.cluster as usize,
                    font_id: font_id,
                    codepoint: info.codepoint,
                    width: 0.0,
                    height: 0.0,
                    advance_x: position.x_advance as f32 * scale,
                    advance_y: position.y_advance as f32 * scale,
                    offset_x: position.x_offset as f32 * scale,
                    offset_y: position.y_offset as f32 * scale,
                    bearing_x: 0.0,
                    bearing_y: 0.0,
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
        })
    }

    // Calculates the x,y coordinates for each glyph based on their advances. Calculates total width and height of the shaped text run
    fn layout(x: f32, y: f32, fontdb: &mut FontDb, res: &mut TextMetrics, paint: &Paint) -> Result<(), ErrorKind> {
        let mut cursor_x = x;
        let mut cursor_y = y;

        // Horizontal alignment
        match paint.text_align {
            Align::Center => cursor_x -= res.width / 2.0,
            Align::Right => cursor_x -= res.width,
            _ => (),
        }

        res.x = cursor_x;

        let mut min_y = cursor_y;
        let mut max_y = cursor_y;

        for glyph in &mut res.glyphs {
            let font = fontdb.get_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;

            // Baseline alignment
            let metrics = font.metrics(paint.font_size);

            let alignment_offset_y = match paint.text_baseline {
                Baseline::Top => metrics.ascender,
                Baseline::Middle => (metrics.ascender + metrics.descender) / 2.0,
                Baseline::Alphabetic => 0.0,
                Baseline::Bottom => metrics.descender,
            };

            glyph.x = cursor_x + glyph.offset_x + glyph.bearing_x;
            glyph.y = cursor_y + glyph.offset_y - glyph.bearing_y + alignment_offset_y;

            min_y = min_y.min(glyph.y);
            max_y = max_y.max(glyph.y + glyph.height);

            cursor_x += glyph.advance_x + paint.letter_spacing;
            cursor_y += glyph.advance_y;
        }

        res.y = min_y;
        res.height = max_y - min_y;

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
