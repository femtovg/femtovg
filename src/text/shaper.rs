
use harfbuzz_rs as hb;
use self::hb::hb as hb_sys;
use self::hb::UnicodeBuffer;

use super::{
    Font,
    FontDb,
    FontId,
    TextStyle,
    freetype as ft
};

mod run_segmentation;
use run_segmentation::{
    Segment,
    Segmentable
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

#[derive(Copy, Clone, Debug)]
pub struct ShapedGlyph {
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

#[derive(Clone, Debug)]
pub enum ShapingResult {
    Success(FontId, Vec<(usize, char, hb::GlyphInfo, hb::GlyphPosition)>),
    Fail(usize, Segment)
}

pub struct Shaper {
}

impl Shaper {
    pub fn new() -> Self {
        Self {
        }
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

    pub fn shape<'a>(&mut self, fontdb: &'a mut FontDb, style: TextStyle, text: &str) -> Vec<ShapedGlyph> {
        let mut result = Vec::new();

        // Layout text for the requested font in style
        let mut shaping_results = self.shape_requested_font(fontdb, style, text);

        // for each of the failed runs of text find a fallback font that will render them
        for res in &mut shaping_results {
            if let ShapingResult::Fail(start_index, segment) = res {
                let font = match fontdb.fallback(&style, &segment.text) {
                    Ok(font) => font,
                    Err(_) => {
                        println!("Could not find font");
                        continue;
                    }
                };

                font.set_size(style.size);

                let font_id = font.id;

                let hb_font = Self::hb_font(font);
                let buffer = segment.hb_buffer();

                let output = hb::shape(&hb_font, buffer, &[]);
                let positions = output.get_glyph_positions();
                let infos = output.get_glyph_infos();

                let mut glyphs = Vec::new();

                for (position, (info, (idx, c))) in positions.iter().zip(infos.iter().zip(segment.text.char_indices())) {
                    glyphs.push((*start_index + idx, c, *info, *position));
                }

                *res = ShapingResult::Success(font_id, glyphs);
            }

            if let ShapingResult::Success(font_id, glyph_infos) = res {
                for (index, c, info, position) in glyph_infos {
                    let font = fontdb.get_mut(*font_id).unwrap();
                    font.set_size(style.size);

                    // TODO: Error handling
                    let _ = font.face.load_glyph(info.codepoint, ft::LoadFlag::DEFAULT | ft::LoadFlag::NO_HINTING);
                    let metrics = font.face.glyph().metrics();

                    result.push(ShapedGlyph {
                        c: *c,
                        index: *index,
                        font_id: *font_id,
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
            }
        }

        result
    }

    fn shape_requested_font(&mut self, fontdb: &mut FontDb, style: TextStyle, text: &str) -> Vec<ShapingResult> {
        let mut result = Vec::new();

        // requested font
        let font = match fontdb.find(&style) {
            Ok(font) => font,
            Err(_) => return result,
        };

        font.set_size(style.size);

        let font_id = font.id;

        let hb_font = Self::hb_font(font);

        let mut index = 0;

        // segment the text in runs of the same direction and script
        for segment in text.segments() {
            let buffer = segment.hb_buffer();

            let output = hb::shape(&hb_font, buffer, &[]);
            let positions = output.get_glyph_positions();
            let infos = output.get_glyph_infos();

            // Separate the result into clusters and mark which one of them has missing glyphs
            let mut clusters = Vec::new();
            let mut current_cluster = Vec::new();
            let mut current_cluster_index = -1;
            let mut current_cluster_has_missing = false;

            for (position, (info, c)) in positions.iter().zip(infos.iter().zip(segment.text.chars())) {
                if current_cluster_index != info.cluster as i32 {
                    let cluster = std::mem::replace(&mut current_cluster, Vec::new());
                    if !cluster.is_empty() {
                        clusters.push((current_cluster_has_missing, cluster));
                    }
                    current_cluster_has_missing = false;
                    current_cluster_index = info.cluster as i32;
                }

                current_cluster.push((index, c, *info, *position));

                index += c.len_utf8();

                if info.codepoint == 0 {
                    current_cluster_has_missing = true;
                }
            }

            clusters.push((current_cluster_has_missing, current_cluster));

            // Combine the clusters into runs of successful and unsuccsesful shaping resutls
            if !clusters.is_empty() {
                let (has_missing, items) = clusters.remove(0);

                // determine first result
                let mut current_res = if has_missing {
                    let start_index = items.iter().nth(0).unwrap().0;
                    ShapingResult::Fail(start_index, Segment {
                        text: items.iter().map(|(_, c, _, _)| c).collect(),
                        ..segment
                    })
                } else {
                    ShapingResult::Success(font_id, items)
                };

                // collect the rest of the clusters in results
                for (has_missing, mut items) in clusters {
                    if let ShapingResult::Success(id, mut infos) = current_res {
                        if has_missing {
                            result.push(ShapingResult::Success(id, infos));
                            let start_index = items.iter().nth(0).unwrap().0;
                            current_res = ShapingResult::Fail(start_index, Segment {
                                text: items.iter().map(|(_, c, _, _)| c).collect(),
                                ..segment
                            });
                        } else {
                            infos.append(&mut items);
                            current_res = ShapingResult::Success(font_id, infos);
                        }
                    } else {
                        if let ShapingResult::Fail(start_index, mut segment) = current_res {
                            if has_missing {
                                items.iter().for_each(|(_, c, _, _)| segment.text.push(*c));
                                current_res = ShapingResult::Fail(start_index, segment);
                            } else {
                                result.push(ShapingResult::Fail(start_index, segment));
                                current_res = ShapingResult::Success(font_id, items);
                            }
                        }
                    }
                }

                result.push(current_res);
            }
        }

        result
    }
}
