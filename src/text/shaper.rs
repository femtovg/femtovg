
use std::str::Chars;
use std::iter::Peekable;
use std::hash::{Hash, Hasher};

use unicode_script::{Script, UnicodeScript};
use unicode_bidi::{bidi_class, BidiClass};

use harfbuzz_rs as hb;
use self::hb::hb as hb_sys;

use lru::LruCache;
use fnv::{FnvHasher, FnvBuildHasher};

use super::{
    Align,
    Baseline,
    Weight,
    WidthClass,
    FontStyle,
    Font,
    FontDb,
    FontId,
    fontdb::FontDbError,
    TextStyle,
    freetype as ft,
    RenderStyle,
    TextLayout,
    GLYPH_PADDING
};

const LRU_CACHE_CAPACITY: usize = 1000;

// harfbuzz-sys doesn't add this symbol for mac builds.
// And we need it since we're using freetype on OSX.
extern "C" {
    pub fn hb_ft_font_create_referenced(face: ft::ffi::FT_Face) -> *mut hb_sys::hb_font_t;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Direction {
    Ltr, Rtl
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ShapedGlyph {
    pub x: f32,
    pub y: f32,
    pub c: char,
    pub index: usize,
    pub font_id: FontId,
    pub codepoint: u32,
    pub width: f32,
    pub height: f32,
    pub advance_x: f32,
    pub advance_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub bearing_x: f32,
    pub bearing_y: f32
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
    pub fn new(style: &TextStyle, text: &str) -> Self {
        let mut hasher = FnvHasher::default();
        text.hash(&mut hasher);

        ShapingId {
            size: style.size,
            text_hash: hasher.finish(),
            weight: style.weight,
            width_class: style.width_class,
            font_style: style.font_style,
        }
    }
}

pub struct Shaper {
    cache: LruCache<ShapingId, Result<(ShapedGlyph, Vec<ShapedGlyph>), FontDbError>, FnvBuildHasher>
}

impl Shaper {
    pub fn new() -> Self {
        let fnv = FnvBuildHasher::default();

        Self {
            cache: LruCache::with_hasher(LRU_CACHE_CAPACITY, fnv)
        }
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn shape(&mut self, x: f32, y: f32, fontdb: &mut FontDb, style: &TextStyle, text: &str) -> TextLayout {
        let mut result = TextLayout {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            glyphs: Vec::new()
        };

        // separate text in runs of the continuous script (Latin, Cyrillic, etc.)
        for (script, direction, subtext) in text.unicode_scripts() {
            // separate words in run
            let mut words: Vec<&str> = subtext.split(" ").collect();

            // reverse the words in right-to-left scripts bit not the trailing whitespace
            if direction == Direction::Rtl {
                let mut rev_range = words.len();

                for (i, word) in words.iter().enumerate().rev() {
                    if word.is_empty() {
                        rev_range = i;
                    } else {
                        break;
                    }
                }

                words[0..rev_range].reverse();
            }

            let mut words_glyphs = Vec::new();

            // we need the space glyph to join the words after they are shaped
            let mut space_glyph = None;

            // shape each word and cache the generated glyphs
            for word in words {

                let shaping_id = ShapingId::new(style, word);

                if self.cache.peek(&shaping_id).is_none() {
                    let ret = fontdb.find_font(&word, style, |font| {
                        font.set_size(style.size);

                        // Call harfbuzz
                        let output = {
                            //let kern = hb::Feature::new(hb::Tag::new('k', 'e', 'r', 'n'), 0, 0..);

                            let hb_font = Self::hb_font(font);
                            let buffer = Self::hb_buffer(&word, &direction, &script);
                            //hb::shape(&hb_font, buffer, &[kern])
                            hb::shape(&hb_font, buffer, &[])
                        };

                        let positions = output.get_glyph_positions();
                        let infos = output.get_glyph_infos();

                        let mut items = Vec::new();

                        let mut has_missing = false;

                        for (position, (info, c)) in positions.iter().zip(infos.iter().zip(word.chars())) {
                            if info.codepoint == 0 {
                                has_missing = true;
                            }

                            let _ = font.face.load_glyph(info.codepoint, ft::LoadFlag::DEFAULT);
                            let metrics = font.face.glyph().metrics();

                            items.push(ShapedGlyph {
                                x: 0.0,
                                y: 0.0,
                                c: c,
                                index: 0,
                                font_id: font.id,
                                codepoint: info.codepoint,
                                width: metrics.width as f32 / 64.0,
                                height: metrics.height as f32 / 64.0,
                                advance_x: position.x_advance as f32 / 64.0,
                                advance_y: position.y_advance as f32 / 64.0,
                                offset_x: position.x_offset as f32 / 64.0,
                                offset_y: position.y_offset as f32 / 64.0,
                                bearing_x: metrics.horiBearingX as f32 / 64.0,
                                bearing_y: metrics.horiBearingY as f32 / 64.0,
                            });
                        }

                        let space_glyph = Self::space_glyph(font, style);

                        (has_missing, (space_glyph, items))
                    });

                    self.cache.put(shaping_id, ret);
                }

                let result = self.cache.get(&shaping_id).unwrap();

                if let Ok((aspace_glyph, items)) = result {
                    words_glyphs.push(items.clone());
                    space_glyph = Some(*aspace_glyph);
                }
            }

            if let Some(space_glyph) = space_glyph {
                result.glyphs.append(&mut words_glyphs.join(&space_glyph));
            } else {
                let mut flat = words_glyphs.into_iter().flatten().collect();
                result.glyphs.append(&mut flat);
            }
        }

        self.layout(x, y, fontdb, &mut result, &style);

        result
    }

    fn layout(&mut self, x: f32, y: f32, fontdb: &mut FontDb, res: &mut TextLayout, style: &TextStyle<'_>) {
        let mut cursor_x = x;
        let mut cursor_y = y;

        let mut padding = GLYPH_PADDING + style.blur.ceil() as u32;

        let line_width = if let RenderStyle::Stroke { width } = style.render_style {
            padding += width as u32;
            width
        } else {
            0
        };

        // calculate total advance
        res.width = res.glyphs.iter().fold(0.0, |width, glyph| width + glyph.advance_x + style.letter_spacing);

        match style.align {
            Align::Center => cursor_x -= res.width / 2.0,
            Align::Right => cursor_x -= res.width,
            _ => ()
        }

        res.x = cursor_x;

        // TODO: Error handling

        let mut height = 0.0f32;
        let mut y = cursor_y;

        for glyph in &mut res.glyphs {
            let font = fontdb.get_mut(glyph.font_id).unwrap();
            font.set_size(style.size);

            let xpos = cursor_x + glyph.offset_x + glyph.bearing_x - (padding as f32) - (line_width as f32) / 2.0;
            let ypos = cursor_y + glyph.offset_y - glyph.bearing_y - (padding as f32) - (line_width as f32) / 2.0;

            // Baseline alignment
            let size_metrics = font.face.size_metrics().unwrap();
            let ascender = size_metrics.ascender as f32 / 64.0;
            let descender = size_metrics.descender as f32 / 64.0;

            let offset_y = match style.baseline {
                Baseline::Top => ascender,
                Baseline::Middle => (ascender + descender) / 2.0,
                Baseline::Alphabetic => 0.0,
                Baseline::Bottom => descender,
            };

            height = height.max(size_metrics.height as f32 / 64.0);
            //height = size_metrics.height as f32 / 64.0;
            y = y.min(ypos + offset_y);

            glyph.x = xpos.floor();
            glyph.y = (ypos + offset_y).floor();

            cursor_x += glyph.advance_x + style.letter_spacing;
            cursor_y += glyph.advance_y;
        }

        res.y = y;
        res.height = height;
    }

    fn space_glyph(font: &mut Font, style: &TextStyle) -> ShapedGlyph {
        let mut glyph = ShapedGlyph::default();

        font.set_size(style.size);

        let index = font.face.get_char_index(' ' as u32);
        let _ = font.face.load_glyph(index, ft::LoadFlag::DEFAULT);
        let metrics = font.face.glyph().metrics();

        glyph.font_id = font.id;
        glyph.c = ' ';
        glyph.codepoint = index;
        glyph.advance_x = metrics.horiAdvance as f32 / 64.0;

        glyph
    }

    fn hb_font(font: &mut Font) -> hb::Owned<hb::Font> {
        // harfbuzz_rs doesn't provide a safe way of creating Face or a Font from a freetype face
        // And I didn't want to read the file a second time and keep it in memory just to give
        // it to harfbuzz_rs here. hb::Owned will free the pointer correctly.

        unsafe {
            let raw_font = hb_ft_font_create_referenced(font.face.raw_mut());
            hb::Owned::from_raw(raw_font)
        }
    }

    fn hb_buffer(text: &str, direction: &Direction, script: &Script) -> hb::UnicodeBuffer {
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

// Segmentation

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

// TODO: Make this borrow a &str instead of allocating a String every time
pub struct UnicodeScriptIterator<I: Iterator<Item = char>> {
    iter: Peekable<I>
}

impl<I: Iterator<Item = char>> Iterator for UnicodeScriptIterator<I> {
    type Item = (Script, Direction, String);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(first) = self.iter.next() {
            let direction = Direction::from(bidi_class(first));
            let mut script = first.script();
            let mut text = String::new();
            text.push(first);

            while let Some(next) = self.iter.peek() {
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
                    text.push(self.iter.next().unwrap());
                } else {
                    break;
                }
            }

            return Some((script, direction, text));
        }

        None
    }
}

pub trait UnicodeScripts<I: Iterator<Item = char>> {
    fn unicode_scripts(self) -> UnicodeScriptIterator<I>;
}

impl<'a> UnicodeScripts<Chars<'a>> for &'a str {
    fn unicode_scripts(self) -> UnicodeScriptIterator<Chars<'a>> {
        UnicodeScriptIterator {
            iter: self.chars().peekable()
        }
    }
}

impl<I: Iterator<Item=char>> UnicodeScripts<I> for I {
    fn unicode_scripts(self) -> UnicodeScriptIterator<I> {
        UnicodeScriptIterator {
            iter: self.peekable()
        }
    }
}
