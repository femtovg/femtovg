
use unicode_script::Script;

use harfbuzz_rs as hb;
use self::hb::hb as hb_sys;
use unicode_bidi::BidiInfo;

use super::{
    Align,
    Baseline,
    Font,
    FontDb,
    FontId,
    TextStyle,
    freetype as ft,
    RenderStyle,
    TextLayout,
    GLYPH_PADDING
};

mod run_segmentation;
use run_segmentation::{
    Segment,
    Segmentable,
    UnicodeScripts,
};

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

pub struct Shaper {
}

impl Shaper {
    pub fn new() -> Self {
        Self {
        }
    }

    pub fn layout(&mut self, x: f32, y: f32, fontdb: &mut FontDb, res: &mut TextLayout, style: &TextStyle<'_>) {
        let mut cursor_x = x;
        let mut cursor_y = y;

        let mut padding = GLYPH_PADDING + style.blur.ceil() as u32;

        let line_width = if let RenderStyle::Stroke { width } = style.render_style {
            padding += width as u32;
            width
        } else {
            0
        };

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

            height = height.max(ascender - descender);
            y = y.min(ypos + offset_y);

            glyph.x = xpos;
            glyph.y = ypos + offset_y;

            cursor_x += glyph.advance_x + style.letter_spacing;
            cursor_y += glyph.advance_y;
        }

        res.y = y;
        res.height = height;
    }

    pub fn shape(&mut self, x: f32, y: f32, fontdb: &mut FontDb, style: &TextStyle, text: &str) -> TextLayout {
        let mut result = TextLayout {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            glyphs: Vec::new()
        };

        let space_glyph = Self::space_glyph(fontdb, style);

        for (script, direction, subtext) in text.unicode_scripts() {
            let mut words: Vec<&str> = subtext.split(" ").collect();

            if direction == Direction::Rtl {
                words.reverse();
            }

            let mut words_glyphs = Vec::new();

            for word in words {
                'fonts: for font in fontdb.fonts_for(&word, style) {
                    font.set_size(style.size);

                    let output = {
                        let hb_font = Self::hb_font(font);
                        let buffer = Self::hb_buffer(&word, &direction, &script);
                        hb::shape(&hb_font, buffer, &[])
                    };

                    let positions = output.get_glyph_positions();
                    let infos = output.get_glyph_infos();

                    let mut items = Vec::new();

                    for (position, (info, c)) in positions.iter().zip(infos.iter().zip(word.chars())) {
                        if info.codepoint == 0 {
                            continue 'fonts;
                        }

                        let _ = font.face.load_glyph(info.codepoint, ft::LoadFlag::DEFAULT | ft::LoadFlag::NO_HINTING);
                        let metrics = font.face.glyph().metrics();

                        let advance_x = position.x_advance as f32 / 64.0;

                        result.width += advance_x;

                        items.push(ShapedGlyph {
                            x: 0.0,
                            y: 0.0,
                            c: c,
                            index: 0,
                            font_id: font.id,
                            codepoint: info.codepoint,
                            width: metrics.width as f32 / 64.0,
                            height: metrics.height as f32 / 64.0,
                            advance_x: advance_x,
                            advance_y: position.y_advance as f32 / 64.0,
                            offset_x: position.x_offset as f32 / 64.0,
                            offset_y: position.y_offset as f32 / 64.0,
                            bearing_x: metrics.horiBearingX as f32 / 64.0,
                            bearing_y: metrics.horiBearingY as f32 / 64.0,
                        });
                    }

                    words_glyphs.push(items);

                    break;
                }
            }

            result.glyphs.append(&mut words_glyphs.join(&space_glyph));
        }

        self.layout(x, y, fontdb, &mut result, &style);

        result
    }

    fn space_glyph(fontdb: &mut FontDb, style: &TextStyle) -> ShapedGlyph {
        let mut glyph = ShapedGlyph::default();

        for font in fontdb.fonts_for(" ", style) {
            font.set_size(style.size);

            let index = font.face.get_char_index(' ' as u32);
            let _ = font.face.load_glyph(index, ft::LoadFlag::DEFAULT | ft::LoadFlag::NO_HINTING);
            let metrics = font.face.glyph().metrics();

            glyph.font_id = font.id;
            glyph.c = ' ';
            glyph.codepoint = index;
            glyph.advance_x = metrics.horiAdvance as f32 / 64.0;
        }

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
