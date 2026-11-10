//! Text: usvg's glyph outlines as paths by default, or through femtovg's text engine.

use std::{collections::HashMap, sync::Arc};

use femtovg::{FontId, Paint, PositionedGlyph};
use usvg::fontdb;

use crate::scene::{ts_to_t2d, Canvas, Painter};

/// How text is drawn. Text the femtovg modes cannot map is drawn as outlines and counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextMode {
    /// usvg's layout flattened to its glyph outlines, the fonts' own outlines as a browser draws them.
    #[default]
    Outlines,
    /// usvg's shaping and positions, each glyph run through `fill_glyph_run`/`stroke_glyph_run`: femtovg's
    /// rasterizer, glyph atlas and colour glyphs.
    Glyphs,
    /// Each chunk's text through `fill_text`/`stroke_text`: femtovg's shaping, bidi and font fallback too.
    Shape,
}

/// The scene's font database and the femtovg faces made of its faces so far, with their units per em.
#[derive(Default)]
pub struct Fonts {
    pub db: Option<Arc<fontdb::Database>>,
    pub context: femtovg::TextContext,
    faces: HashMap<fontdb::ID, Option<(FontId, u16)>>,
}

impl Fonts {
    /// The femtovg face of a fontdb face, registered with the text context on first use.
    fn face(&mut self, id: fontdb::ID) -> Option<(FontId, u16)> {
        if let Some(face) = self.faces.get(&id) {
            return *face;
        }
        let face = self.db.as_ref().and_then(|db| {
            let (data, index) = db.with_face_data(id, |data, index| (data.to_vec(), index))?;
            let upem = units_per_em(&data, index)?;
            let font = self.context.add_shared_font_with_index(data, index).ok()?;
            Some((font, upem))
        });
        self.faces.insert(id, face);
        face
    }
}

/// A face's units per em, from its `head` table (in a font or a collection).
fn units_per_em(data: &[u8], index: u32) -> Option<u16> {
    let be16 = |o: usize| Some(u16::from_be_bytes(data.get(o..o + 2)?.try_into().ok()?));
    let be32 = |o: usize| Some(u32::from_be_bytes(data.get(o..o + 4)?.try_into().ok()?));
    let dir = if data.get(0..4) == Some(b"ttcf".as_slice()) {
        be32(12 + 4 * index as usize)? as usize
    } else {
        0
    };
    (0..be16(dir + 4)? as usize)
        .map(|i| dir + 12 + 16 * i)
        .find(|&rec| data.get(rec..rec + 4) == Some(b"head".as_slice()))
        .and_then(|rec| be16(be32(rec + 8)? as usize + 18))
}

/// Draws one run of glyphs of one face and size; false when femtovg returned an error.
fn glyph_run(
    canvas: &mut Canvas,
    face: FontId,
    size: f32,
    run: &[(f32, f32, u16)],
    paint: &mut Paint,
    stroked: bool,
) -> bool {
    let glyphs = run.iter().map(|&(x, y, glyph_id)| PositionedGlyph { x, y, glyph_id });
    paint.set_font(&[face]);
    paint.set_font_size(size);
    let drawn = if stroked {
        canvas.stroke_glyph_run(face, &[], glyphs, paint)
    } else {
        canvas.fill_glyph_run(face, &[], glyphs, paint)
    };
    drawn.is_ok()
}

/// One chunk of `TextMode::Shape` text, ready to draw.
struct ShapedChunk {
    x: f32,
    y: f32,
    text: String,
    anchor: usvg::TextAnchor,
    size: f32,
    letter_spacing: f32,
    faces: Vec<FontId>,
    passes: Vec<(Paint, f32, bool)>,
}

impl Painter {
    pub(crate) fn draw_text(&mut self, canvas: &mut Canvas, text: &usvg::Text) {
        let drawn = match self.opts.text {
            TextMode::Outlines => false,
            TextMode::Glyphs => self.draw_text_glyphs(canvas, text),
            TextMode::Shape => self.draw_text_shaped(canvas, text),
        };
        if drawn {
            self.stats.text_femtovg += 1;
            return;
        }
        self.stats.text_outlines += 1;
        for child in text.flattened().children() {
            match child {
                // A colour glyph (bitmap, SVG or COLR) is a group whose own transform places it; usvg leaves the
                // abs_transform of the group and everything under it at identity, so the subtree is drawn under the
                // text's transform and the group's.
                usvg::Node::Group(glyph) => {
                    let outer = self.abs_base;
                    self.abs_base = outer.pre_concat(text.abs_transform().pre_concat(glyph.transform()));
                    self.draw_nodes(canvas, glyph.children());
                    self.abs_base = outer;
                }
                _ => self.draw_nodes(canvas, std::slice::from_ref(child)),
            }
        }
    }

    /// `TextMode::Glyphs`. A glyph usvg places by more than a translation (`rotate`, a text path, `lengthAdjust`,
    /// vertical text) is drawn alone under that placement.
    fn draw_text_glyphs(&mut self, canvas: &mut Canvas, text: &usvg::Text) -> bool {
        let spans = text.layouted();
        if spans
            .iter()
            .flat_map(|s| &s.positioned_glyphs)
            .any(|g| self.fonts.face(g.font).is_none())
        {
            return false;
        }
        let transform = text.abs_transform();
        for span in spans.iter().filter(|s| s.visible) {
            let lines: Vec<usvg::Node> = [&span.overline, &span.underline]
                .into_iter()
                .flatten()
                .map(|p| usvg::Node::Path(Box::new(p.clone())))
                .collect();
            self.draw_nodes(canvas, &lines);
            for (mut paint, alpha, stroked) in
                self.span_passes(span.fill.as_ref(), span.stroke.as_ref(), span.paint_order)
            {
                canvas.save();
                self.set_node_transform(canvas, transform);
                canvas.set_global_alpha(alpha);
                let mut errors = 0;
                // Runs of glyphs of one face and size placed by translations.
                let mut run: Vec<(f32, f32, u16)> = Vec::new();
                let mut run_key = None;
                for g in &span.positioned_glyphs {
                    let (face, upem) = self.fonts.face(g.font).expect("checked above");
                    let size = g.font_size();
                    let k = size / upem as f32;
                    let at = g.transform().pre_scale(1.0 / k, 1.0 / k);
                    let placed = (at.sx - 1.0).abs() < 1e-4
                        && (at.sy - 1.0).abs() < 1e-4
                        && at.kx.abs() < 1e-4
                        && at.ky.abs() < 1e-4;
                    if run_key != Some((face, size.to_bits())) || !placed {
                        if let Some((face, size)) = run_key.take() {
                            errors += usize::from(!glyph_run(
                                canvas,
                                face,
                                f32::from_bits(size),
                                &run,
                                &mut paint,
                                stroked,
                            ));
                        }
                        run.clear();
                    }
                    if placed {
                        run_key = Some((face, size.to_bits()));
                        run.push((at.tx, at.ty, g.id.0 as u16));
                    } else {
                        canvas.save();
                        canvas.set_transform(&ts_to_t2d(at));
                        errors += usize::from(!glyph_run(
                            canvas,
                            face,
                            size,
                            &[(0.0, 0.0, g.id.0 as u16)],
                            &mut paint,
                            stroked,
                        ));
                        canvas.restore();
                    }
                }
                if let Some((face, size)) = run_key {
                    errors += usize::from(!glyph_run(
                        canvas,
                        face,
                        f32::from_bits(size),
                        &run,
                        &mut paint,
                        stroked,
                    ));
                }
                self.stats.text_errors += errors;
                canvas.restore();
            }
            let lines: Vec<usvg::Node> = span
                .line_through
                .iter()
                .map(|p| usvg::Node::Path(Box::new(p.clone())))
                .collect();
            self.draw_nodes(canvas, &lines);
        }
        true
    }

    /// `TextMode::Shape`: each chunk at its own x and y with its anchor, in its spans' fonts (each family of theirs
    /// that resolves, then the faces usvg's layout fell back to). Maps horizontal, unpositioned, undecorated chunks
    /// whose spans share one style; anything else is left to usvg.
    fn draw_text_shaped(&mut self, canvas: &mut Canvas, text: &usvg::Text) -> bool {
        if text.writing_mode() != usvg::WritingMode::LeftToRight
            || text
                .dx()
                .iter()
                .chain(text.dy())
                .chain(text.rotate())
                .any(|v| *v != 0.0)
        {
            return false;
        }
        let mut fallback = Vec::new();
        for g in text.layouted().iter().flat_map(|s| &s.positioned_glyphs) {
            match self.fonts.face(g.font) {
                Some((face, _)) if !fallback.contains(&face) => fallback.push(face),
                Some(_) => {}
                None => return false,
            }
        }
        let Some(db) = self.fonts.db.clone() else {
            return false;
        };
        let style_of = |s: &usvg::TextSpan| {
            format!(
                "{:?}|{:?}|{:?}|{:?}|{}|{}|{:?}",
                s.font(),
                s.font_size(),
                s.fill(),
                s.stroke(),
                s.letter_spacing(),
                s.apply_kerning(),
                s.paint_order()
            )
        };
        let plain = |s: &usvg::TextSpan| {
            s.is_visible()
                && s.baseline_shift().is_empty()
                && matches!(
                    s.dominant_baseline(),
                    usvg::DominantBaseline::Auto | usvg::DominantBaseline::Alphabetic
                )
                && matches!(
                    s.alignment_baseline(),
                    usvg::AlignmentBaseline::Auto | usvg::AlignmentBaseline::Baseline
                )
                && !s.small_caps()
                && s.word_spacing() == 0.0
                && s.text_length().is_none()
                && s.decoration().underline().is_none()
                && s.decoration().overline().is_none()
                && s.decoration().line_through().is_none()
                // usvg states the weight as a `wght` variation; any other variation would need the font's
                // normalized coordinates.
                && s.font().variations().iter().all(|v| &v.tag == b"wght" && v.value == f32::from(s.font().weight()))
        };
        let mut chunks = Vec::new();
        for chunk in text.chunks() {
            let (Some(x), Some(y), [span, rest @ ..]) = (chunk.x(), chunk.y(), chunk.spans()) else {
                return false;
            };
            if !matches!(chunk.text_flow(), usvg::TextFlow::Linear)
                || !plain(span)
                || rest.iter().any(|s| !plain(s) || style_of(s) != style_of(span))
            {
                return false;
            }
            let font = span.font();
            let mut faces = Vec::new();
            for family in font.families() {
                let family = match family {
                    usvg::FontFamily::Serif => fontdb::Family::Serif,
                    usvg::FontFamily::SansSerif => fontdb::Family::SansSerif,
                    usvg::FontFamily::Cursive => fontdb::Family::Cursive,
                    usvg::FontFamily::Fantasy => fontdb::Family::Fantasy,
                    usvg::FontFamily::Monospace => fontdb::Family::Monospace,
                    usvg::FontFamily::Named(name) => fontdb::Family::Name(name),
                };
                let query = fontdb::Query {
                    families: &[family],
                    weight: fontdb::Weight(font.weight()),
                    stretch: match font.stretch() {
                        usvg::FontStretch::UltraCondensed => fontdb::Stretch::UltraCondensed,
                        usvg::FontStretch::ExtraCondensed => fontdb::Stretch::ExtraCondensed,
                        usvg::FontStretch::Condensed => fontdb::Stretch::Condensed,
                        usvg::FontStretch::SemiCondensed => fontdb::Stretch::SemiCondensed,
                        usvg::FontStretch::Normal => fontdb::Stretch::Normal,
                        usvg::FontStretch::SemiExpanded => fontdb::Stretch::SemiExpanded,
                        usvg::FontStretch::Expanded => fontdb::Stretch::Expanded,
                        usvg::FontStretch::ExtraExpanded => fontdb::Stretch::ExtraExpanded,
                        usvg::FontStretch::UltraExpanded => fontdb::Stretch::UltraExpanded,
                    },
                    style: match font.style() {
                        usvg::FontStyle::Normal => fontdb::Style::Normal,
                        usvg::FontStyle::Italic => fontdb::Style::Italic,
                        usvg::FontStyle::Oblique => fontdb::Style::Oblique,
                    },
                };
                if let Some((face, _)) = db.query(&query).and_then(|id| self.fonts.face(id)) {
                    if !faces.contains(&face) {
                        faces.push(face);
                    }
                }
            }
            for face in &fallback {
                if !faces.contains(face) {
                    faces.push(*face);
                }
            }
            chunks.push(ShapedChunk {
                x,
                y,
                text: chunk.text().to_string(),
                anchor: chunk.anchor(),
                size: span.font_size().get(),
                letter_spacing: span.letter_spacing(),
                faces,
                passes: self.span_passes(span.fill(), span.stroke(), span.paint_order()),
            });
        }
        let transform = text.abs_transform();
        for chunk in chunks {
            for (mut paint, alpha, stroked) in chunk.passes {
                paint.set_font(&chunk.faces);
                paint.set_font_size(chunk.size);
                paint.set_letter_spacing(chunk.letter_spacing);
                paint.set_text_baseline(femtovg::Baseline::Alphabetic);
                paint.set_text_align(match chunk.anchor {
                    usvg::TextAnchor::Start => femtovg::Align::Left,
                    usvg::TextAnchor::Middle => femtovg::Align::Center,
                    usvg::TextAnchor::End => femtovg::Align::Right,
                });
                canvas.save();
                self.set_node_transform(canvas, transform);
                canvas.set_global_alpha(alpha);
                let drawn = if stroked {
                    canvas.stroke_text(chunk.x, chunk.y, &chunk.text, &paint).map(drop)
                } else {
                    canvas.fill_text(chunk.x, chunk.y, &chunk.text, &paint).map(drop)
                };
                self.stats.text_errors += usize::from(drawn.is_err());
                canvas.restore();
            }
        }
        true
    }
}
