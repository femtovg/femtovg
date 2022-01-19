use std::cell::RefCell;
use std::ffi::OsStr;
use std::fs;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::Path as FilePath;
use std::rc::Rc;

use fnv::{FnvBuildHasher, FnvHashMap, FnvHasher};
use generational_arena::{Arena, Index};
use lru::LruCache;

use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Canvas, Color, ErrorKind, FillRule, ImageFlags, ImageId, ImageInfo, Paint, PixelFormat, RenderTarget, Renderer,
};

mod atlas;
pub use atlas::Atlas;

mod font;
use font::Font;
pub use font::FontMetrics;

use self::font::GlyphRendering;

// This padding is an empty border around the glyph’s pixels but inside the
// sampled area (texture coordinates) for the quad in render_atlas().
const GLYPH_PADDING: u32 = 1;
// We add an additional margin of 1 pixel outside of the sampled area,
// to deal with the linear interpolation of texels at the edge of that area
// which mixes in the texels just outside of the edge.
// This manifests as noise around the glyph, outside of the padding.
const GLYPH_MARGIN: u32 = 1;

const TEXTURE_SIZE: usize = 512;
const LRU_CACHE_CAPACITY: usize = 1000;

/// A font handle.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FontId(Index);

/// Text baseline vertical alignment:
/// `Top`, `Middle`, `Alphabetic` (default), `Bottom`.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub enum Baseline {
    /// The text baseline is the top of the em square.
    Top,
    /// The text baseline is the middle of the em square.
    Middle,
    /// The text baseline is the normal alphabetic baseline. Default value.
    Alphabetic,
    // The text baseline is the bottom of the bounding box.
    Bottom,
}

impl Default for Baseline {
    fn default() -> Self {
        Self::Alphabetic
    }
}

/// Text horizontal alignment: `Left` (default), `Center`, `Right`.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub enum Align {
    /// The text is left-aligned.
    Left,
    /// The text is centered.
    Center,
    /// The text is right-aligned.
    Right,
}

impl Default for Align {
    fn default() -> Self {
        Self::Left
    }
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum RenderMode {
    Fill,
    Stroke,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::Fill
    }
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub(crate) struct RenderedGlyphId {
    glyph_index: u32,
    font_id: FontId,
    size: u32,
    line_width: u32,
    render_mode: RenderMode,
    subpixel_location: u8,
}

impl RenderedGlyphId {
    fn new(glyph_index: u32, font_id: FontId, paint: &Paint, mode: RenderMode, subpixel_location: u8) -> Self {
        Self {
            glyph_index,
            font_id,
            size: (paint.font_size * 10.0).trunc() as u32,
            line_width: (paint.line_width * 10.0).trunc() as u32,
            render_mode: mode,
            subpixel_location,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct RenderedGlyph {
    texture_index: usize,
    width: u32,
    height: u32,
    bearing_y: i32,
    atlas_x: u32,
    atlas_y: u32,
    color_glyph: bool,
}

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
    pub bitmap_glyph: bool,
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
    fn new(paint: &Paint, word: &str, max_width: Option<f32>) -> Self {
        let mut hasher = FnvHasher::default();
        word.hash(&mut hasher);
        if let Some(max_width) = max_width {
            (max_width.trunc() as i32).hash(&mut hasher);
        }

        Self {
            size: (paint.font_size * 10.0).trunc() as u32,
            word_hash: hasher.finish(),
            font_ids: paint.font_ids,
        }
    }
}

type ShapedWordsCache<H> = LruCache<ShapingId, Result<ShapedWord, ErrorKind>, H>;
type ShapingRunCache<H> = LruCache<ShapingId, TextMetrics, H>;

pub(crate) struct FontTexture {
    atlas: Atlas,
    pub(crate) image_id: ImageId,
}

/// TextContext provides functionality for text processing in femtovg. You can
/// add fonts using the [`Self::add_font_file()`], [`Self::add_font_mem()`] and
/// [`Self::add_font_dir()`] functions. For each registered font a [`FontId`] is
/// returned.
///
/// The [`FontId`] can be supplied to [`crate::Paint`] along with additional parameters
/// such as the font size.
///
/// The paint is needed when using TextContext's measurement functions such as
/// [`Self::measure_text()`].
///
/// Note that the measurements are done entirely with the supplied sizes in the paint
/// parameter. If you need measurements that take a [`crate::Canvas`]'s transform or dpi into
/// account (see [`crate::Canvas::set_size()`]), you need to use the measurement functions
/// on the canvas.
#[derive(Clone)]
pub struct TextContext(pub(crate) Rc<RefCell<TextContextImpl>>);

impl Default for TextContext {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl TextContext {
    /// Registers all .ttf files from a directory with this text context. If successful, the
    /// font ids of all registered fonts are returned.
    pub fn add_font_dir<T: AsRef<FilePath>>(&self, path: T) -> Result<Vec<FontId>, ErrorKind> {
        self.0.as_ref().borrow_mut().add_font_dir(path)
    }

    /// Registers the .ttf file from the specified path with this text context. If successful,
    /// the font id is returned.
    pub fn add_font_file<T: AsRef<FilePath>>(&self, path: T) -> Result<FontId, ErrorKind> {
        self.0.as_ref().borrow_mut().add_font_file(path)
    }

    /// Registers the in-memory representation of a TrueType font pointed to by the data
    /// parameter with this text context. If successful, the font id is returned.
    pub fn add_font_mem(&self, data: &[u8]) -> Result<FontId, ErrorKind> {
        self.0.as_ref().borrow_mut().add_font_mem(data)
    }

    /// Registers the in-memory representation of a TrueType font pointed to by the shared data
    /// parameter with this text context. If successful, the font id is returned. The face_index
    /// specifies the face index if the font data is a true type font collection. For plain true
    /// type fonts, use 0 as index.
    pub fn add_shared_font_with_index<T: AsRef<[u8]> + 'static>(
        &self,
        data: T,
        face_index: u32,
    ) -> Result<FontId, ErrorKind> {
        self.0
            .as_ref()
            .borrow_mut()
            .add_shared_font_with_index(data, face_index)
    }

    /// Returns information on how the provided text will be drawn with the specified paint.
    pub fn measure_text<S: AsRef<str>>(&self, x: f32, y: f32, text: S, paint: Paint) -> Result<TextMetrics, ErrorKind> {
        self.0.as_ref().borrow_mut().measure_text(x, y, text, paint)
    }

    /// Returns the maximum index-th byte of text that will fit inside max_width.
    ///
    /// The retuned index will always lie at the start and/or end of a UTF-8 code point sequence or at the start or end of the text
    pub fn break_text<S: AsRef<str>>(&self, max_width: f32, text: S, paint: Paint) -> Result<usize, ErrorKind> {
        self.0.as_ref().borrow_mut().break_text(max_width, text, paint)
    }

    /// Returnes a list of ranges representing each line of text that will fit inside max_width
    pub fn break_text_vec<S: AsRef<str>>(
        &self,
        max_width: f32,
        text: S,
        paint: Paint,
    ) -> Result<Vec<Range<usize>>, ErrorKind> {
        self.0.as_ref().borrow_mut().break_text_vec(max_width, text, paint)
    }

    /// Returns font metrics for a particular Paint.
    pub fn measure_font(&self, paint: Paint) -> Result<FontMetrics, ErrorKind> {
        self.0.as_ref().borrow_mut().measure_font(paint)
    }
}

pub(crate) struct TextContextImpl {
    fonts: Arena<Font>,
    shaping_run_cache: ShapingRunCache<FnvBuildHasher>,
    shaped_words_cache: ShapedWordsCache<FnvBuildHasher>,
}

impl Default for TextContextImpl {
    fn default() -> Self {
        let fnv_run = FnvBuildHasher::default();
        let fnv_words = FnvBuildHasher::default();

        Self {
            fonts: Default::default(),
            shaping_run_cache: LruCache::with_hasher(LRU_CACHE_CAPACITY, fnv_run),
            shaped_words_cache: LruCache::with_hasher(LRU_CACHE_CAPACITY, fnv_words),
        }
    }
}

impl TextContextImpl {
    pub fn add_font_dir<T: AsRef<FilePath>>(&mut self, path: T) -> Result<Vec<FontId>, ErrorKind> {
        let path = path.as_ref();
        let mut fonts = Vec::new();

        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    self.add_font_dir(&path)?;
                } else {
                    if let Some("ttf") = path.extension().and_then(OsStr::to_str) {
                        fonts.push(self.add_font_file(path)?);
                    } else if let Some("ttc") = path.extension().and_then(OsStr::to_str) {
                        fonts.extend(self.add_font_file_collection(path)?);
                    }
                }
            }
        }

        Ok(fonts)
    }

    pub fn add_font_file<T: AsRef<FilePath>>(&mut self, path: T) -> Result<FontId, ErrorKind> {
        let data = std::fs::read(path)?;

        self.add_font_mem(&data)
    }

    pub fn add_font_file_collection<T: AsRef<FilePath>>(
        &mut self,
        path: T,
    ) -> Result<impl Iterator<Item = FontId> + '_, ErrorKind> {
        let data = std::fs::read(path)?;

        let count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        Ok((0..count).filter_map(move |index| Some(self.add_font_mem_with_index(&data, index).ok()?)))
    }

    pub fn add_font_mem(&mut self, data: &[u8]) -> Result<FontId, ErrorKind> {
        self.add_font_mem_with_index(data, 0)
    }

    pub fn add_font_mem_with_index(&mut self, data: &[u8], face_index: u32) -> Result<FontId, ErrorKind> {
        self.clear_caches();

        let font = Font::new_with_data(data.to_owned(), face_index)?;
        Ok(FontId(self.fonts.insert(font)))
    }

    pub fn add_shared_font_with_index<T: AsRef<[u8]> + 'static>(
        &mut self,
        data: T,
        face_index: u32,
    ) -> Result<FontId, ErrorKind> {
        self.clear_caches();

        let font = Font::new_with_data(data, face_index)?;
        Ok(FontId(self.fonts.insert(font)))
    }

    pub fn font(&self, id: FontId) -> Option<&Font> {
        self.fonts.get(id.0)
    }

    pub fn font_mut(&mut self, id: FontId) -> Option<&mut Font> {
        self.fonts.get_mut(id.0)
    }

    pub fn find_font<F, T>(&mut self, paint: &Paint, mut callback: F) -> Result<T, ErrorKind>
    where
        F: FnMut((FontId, &mut Font)) -> (bool, T),
    {
        // Try each font in the paint
        for maybe_font_id in paint.font_ids.iter() {
            if let Some(font_id) = maybe_font_id {
                if let Some(font) = self.fonts.get_mut(font_id.0) {
                    let (has_missing, result) = callback((*font_id, font));

                    if !has_missing {
                        return Ok(result);
                    }
                }
            } else {
                break;
            }
        }

        // Try each registered font
        // An optimisation here would be to skip fonts that were tried by the paint
        for (id, font) in &mut self.fonts {
            let (has_missing, result) = callback((FontId(id), font));

            if !has_missing {
                return Ok(result);
            }
        }

        // Just return the first font at this point and let it render .nodef glyphs
        if let Some((id, font)) = self.fonts.iter_mut().next() {
            return Ok(callback((FontId(id), font)).1);
        }

        Err(ErrorKind::NoFontFound)
    }

    fn clear_caches(&mut self) {
        self.shaped_words_cache.clear();
    }

    pub fn measure_text<S: AsRef<str>>(
        &mut self,
        x: f32,
        y: f32,
        text: S,
        paint: Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        Ok(shape(x, y, self, &paint, text.as_ref(), None)?)
    }

    pub fn break_text<S: AsRef<str>>(&mut self, max_width: f32, text: S, paint: Paint) -> Result<usize, ErrorKind> {
        let layout = shape(0.0, 0.0, self, &paint, text.as_ref(), Some(max_width))?;

        Ok(layout.final_byte_index)
    }

    pub fn break_text_vec<S: AsRef<str>>(
        &mut self,
        max_width: f32,
        text: S,
        paint: Paint,
    ) -> Result<Vec<Range<usize>>, ErrorKind> {
        let text = text.as_ref();

        let mut res = Vec::new();
        let mut start = 0;

        while start < text.len() {
            if let Ok(index) = self.break_text(max_width, &text[start..], paint) {
                if index == 0 {
                    break;
                }

                let index = start + index;
                res.push(start..index);
                start += &text[start..index].len();
            } else {
                break;
            }
        }

        Ok(res)
    }

    pub fn measure_font(&mut self, paint: Paint) -> Result<FontMetrics, ErrorKind> {
        if let Some(Some(id)) = paint.font_ids.get(0) {
            if let Some(font) = self.font(*id) {
                return Ok(font.metrics(paint.font_size));
            }
        }

        Err(ErrorKind::NoFontFound)
    }
}

/// Result of a shaping run.
#[derive(Clone, Default, Debug)]
pub struct TextMetrics {
    pub x: f32,
    pub y: f32,
    width: f32,
    height: f32,
    pub glyphs: Vec<ShapedGlyph>,
    pub(crate) final_byte_index: usize,
}

impl TextMetrics {
    pub(crate) fn scale(&mut self, scale: f32) {
        self.x *= scale;
        self.y *= scale;
        self.width *= scale;
        self.height *= scale;

        for glyph in &mut self.glyphs {
            glyph.x *= scale;
            glyph.y *= scale;
            glyph.width *= scale;
            glyph.height *= scale;
        }
    }

    /// width of the glyphs as drawn
    pub fn width(&self) -> f32 {
        self.width
    }

    /// height of the glyphs as drawn
    pub fn height(&self) -> f32 {
        self.height
    }

    pub(crate) fn has_bitmap_glyphs(&self) -> bool {
        self.glyphs.iter().find(|g| g.bitmap_glyph).is_some()
    }
}

// Shaper

pub(crate) fn shape(
    x: f32,
    y: f32,
    context: &mut TextContextImpl,
    paint: &Paint,
    text: &str,
    max_width: Option<f32>,
) -> Result<TextMetrics, ErrorKind> {
    let id = ShapingId::new(paint, text, max_width);

    if !context.shaping_run_cache.contains(&id) {
        let metrics = shape_run(context, paint, text, max_width)?;
        context.shaping_run_cache.put(id, metrics);
    }

    if let Some(mut metrics) = context.shaping_run_cache.get(&id).cloned() {
        layout(x, y, context, &mut metrics, paint)?;

        return Ok(metrics);
    }

    Err(ErrorKind::UnknownError)
}

fn shape_run(
    context: &mut TextContextImpl,
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
                rustybuzz::Direction::RightToLeft
            } else {
                rustybuzz::Direction::LeftToRight
            };

            let mut words = Vec::new();
            let mut word_break_reached = false;
            let mut byte_index = run.start;

            for word in sub_text.split_word_bounds() {
                let id = ShapingId::new(paint, word, max_width);

                if !context.shaped_words_cache.contains(&id) {
                    let word = shape_word(word, hb_direction, context, paint);
                    context.shaped_words_cache.put(id, word);
                }

                if let Some(Ok(word)) = context.shaped_words_cache.get(&id) {
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

    Ok(result)
}

fn shape_word(
    word: &str,
    hb_direction: rustybuzz::Direction,
    context: &mut TextContextImpl,
    paint: &Paint,
) -> Result<ShapedWord, ErrorKind> {
    // find_font will call the closure with each font matching the provided style
    // until a font capable of shaping the word is found
    context.find_font(paint, |(font_id, font)| {
        // Call harfbuzz
        let output = {
            let face = font.face_ref();

            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(word);
            buffer.set_direction(hb_direction);

            rustybuzz::shape(&face, &[], buffer)
        };

        let positions = output.glyph_positions();
        let infos = output.glyph_infos();

        let mut shaped_word = ShapedWord {
            glyphs: Vec::with_capacity(positions.len()),
            width: 0.0,
        };

        let mut has_missing = false;

        for (position, (info, c)) in positions.iter().zip(infos.iter().zip(word.chars())) {
            if info.glyph_id == 0 {
                has_missing = true;
            }

            let scale = font.scale(paint.font_size);

            let mut g = ShapedGlyph {
                x: 0.0,
                y: 0.0,
                c: c,
                byte_index: info.cluster as usize,
                font_id: font_id,
                codepoint: info.glyph_id,
                width: 0.0,
                height: 0.0,
                advance_x: position.x_advance as f32 * scale,
                advance_y: position.y_advance as f32 * scale,
                offset_x: position.x_offset as f32 * scale,
                offset_y: position.y_offset as f32 * scale,
                bearing_x: 0.0,
                bearing_y: 0.0,
                bitmap_glyph: false,
            };

            if let Some(glyph) = font.glyph(info.glyph_id as u16) {
                g.width = glyph.metrics.width * scale;
                g.height = glyph.metrics.height * scale;
                g.bearing_x = glyph.metrics.bearing_x * scale;
                g.bearing_y = glyph.metrics.bearing_y * scale;
                g.bitmap_glyph = glyph.path.is_none();
            }

            shaped_word.width += g.advance_x + paint.letter_spacing;
            shaped_word.glyphs.push(g);
        }

        (has_missing, shaped_word)
    })
}

// Calculates the x,y coordinates for each glyph based on their advances. Calculates total width and height of the shaped text run
fn layout(
    x: f32,
    y: f32,
    context: &mut TextContextImpl,
    res: &mut TextMetrics,
    paint: &Paint,
) -> Result<(), ErrorKind> {
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

    let mut ascender: f32 = 0.;
    let mut descender: f32 = 0.;

    for glyph in &mut res.glyphs {
        let font = context.font_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
        let metrics = font.metrics(paint.font_size);
        ascender = ascender.max(metrics.ascender());
        descender = descender.min(metrics.descender());
    }

    let primary_metrics = context.find_font(paint, |(_, font)| (false, font.metrics(paint.font_size)))?;
    if ascender.abs() < std::f32::EPSILON {
        ascender = primary_metrics.ascender();
    }
    if descender.abs() < std::f32::EPSILON {
        descender = primary_metrics.descender();
    }

    // Baseline alignment
    let alignment_offset_y = match paint.text_baseline {
        Baseline::Top => ascender,
        Baseline::Middle => (ascender + descender) / 2.0,
        Baseline::Alphabetic => 0.0,
        Baseline::Bottom => descender,
    };

    for glyph in &mut res.glyphs {
        glyph.x = cursor_x + glyph.offset_x + glyph.bearing_x;
        glyph.y = (cursor_y + alignment_offset_y).round() + glyph.offset_y - glyph.bearing_y;

        min_y = min_y.min(glyph.y);
        max_y = max_y.max(glyph.y + glyph.height);

        cursor_x += glyph.advance_x + paint.letter_spacing;
        cursor_y += glyph.advance_y;
    }

    res.y = min_y;
    res.height = max_y - min_y;

    Ok(())
}

// Renderer

#[derive(Clone, Debug)]
pub(crate) struct DrawCmd {
    pub image_id: ImageId,
    pub quads: Vec<Quad>,
}

#[derive(Copy, Clone, Default, Debug)]
pub(crate) struct Quad {
    pub x0: f32,
    pub y0: f32,
    pub s0: f32,
    pub t0: f32,
    pub x1: f32,
    pub y1: f32,
    pub s1: f32,
    pub t1: f32,
}

pub(crate) struct GlyphDrawCommands {
    pub(crate) alpha_glyphs: Vec<DrawCmd>,
    pub(crate) color_glyphs: Vec<DrawCmd>,
}

#[derive(Default)]
pub(crate) struct GlyphAtlas {
    pub rendered_glyphs: RefCell<FnvHashMap<RenderedGlyphId, RenderedGlyph>>,
    pub glyph_textures: RefCell<Vec<FontTexture>>,
}

impl GlyphAtlas {
    pub(crate) fn render_atlas<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        text_layout: &TextMetrics,
        paint: &Paint,
        mode: RenderMode,
    ) -> Result<GlyphDrawCommands, ErrorKind> {
        let mut alpha_cmd_map = FnvHashMap::default();
        let mut color_cmd_map = FnvHashMap::default();

        let line_width_offset = if mode == RenderMode::Stroke {
            (paint.line_width / 2.0).ceil()
        } else {
            0.0
        };

        let initial_render_target = canvas.current_render_target;

        for glyph in &text_layout.glyphs {
            let subpixel_location = crate::geometry::quantize(glyph.x.fract(), 0.1) * 10.0;

            let id = RenderedGlyphId::new(glyph.codepoint, glyph.font_id, paint, mode, subpixel_location as u8);

            if !self.rendered_glyphs.borrow().contains_key(&id) {
                let glyph = self.render_glyph(canvas, paint, mode, &glyph)?;

                self.rendered_glyphs.borrow_mut().insert(id, glyph);
            }

            let rendered_glyphs = self.rendered_glyphs.borrow();
            let rendered = rendered_glyphs.get(&id).unwrap();

            if let Some(texture) = self.glyph_textures.borrow().get(rendered.texture_index) {
                let image_id = texture.image_id;
                let size = texture.atlas.size();
                let itw = 1.0 / size.0 as f32;
                let ith = 1.0 / size.1 as f32;

                let cmd_map = if rendered.color_glyph {
                    &mut color_cmd_map
                } else {
                    &mut alpha_cmd_map
                };

                let cmd = cmd_map.entry(rendered.texture_index).or_insert_with(|| DrawCmd {
                    image_id,
                    quads: Vec::new(),
                });

                let mut q = Quad::default();

                let line_width_offset = if rendered.color_glyph { 0. } else { line_width_offset };

                q.x0 = glyph.x.trunc() - line_width_offset - GLYPH_PADDING as f32;
                q.y0 = (glyph.y + glyph.bearing_y).round()
                    - rendered.bearing_y as f32
                    - line_width_offset
                    - GLYPH_PADDING as f32;
                q.x1 = q.x0 + rendered.width as f32;
                q.y1 = q.y0 + rendered.height as f32;

                q.s0 = rendered.atlas_x as f32 * itw;
                q.t0 = rendered.atlas_y as f32 * ith;
                q.s1 = (rendered.atlas_x + rendered.width) as f32 * itw;
                q.t1 = (rendered.atlas_y + rendered.height) as f32 * ith;

                cmd.quads.push(q);
            }
        }

        canvas.set_render_target(initial_render_target);

        Ok(GlyphDrawCommands {
            alpha_glyphs: alpha_cmd_map.drain().map(|(_, cmd)| cmd).collect(),
            color_glyphs: color_cmd_map.drain().map(|(_, cmd)| cmd).collect(),
        })
    }

    fn render_glyph<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        paint: &Paint,
        mode: RenderMode,
        glyph: &ShapedGlyph,
    ) -> Result<RenderedGlyph, ErrorKind> {
        let padding = GLYPH_PADDING + GLYPH_MARGIN;

        let text_context = canvas.text_context.clone();
        let mut text_context = text_context.borrow_mut();

        let (mut maybe_glyph_representation, scale) = {
            let font = text_context.font_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
            let scale = font.scale(paint.font_size);

            let maybe_glyph_representation =
                font.glyph_rendering_representation(glyph.codepoint as u16, paint.font_size as u16);
            (maybe_glyph_representation, scale)
        };

        #[cfg(feature = "image-loading")]
        let color_glyph = matches!(maybe_glyph_representation, Some(GlyphRendering::RenderAsImage(..)));
        #[cfg(not(feature = "image-loading"))]
        let color_glyph = false;

        let line_width = if color_glyph || mode != RenderMode::Stroke {
            0.0
        } else {
            paint.line_width
        };

        let line_width_offset = (line_width / 2.0).ceil();

        let width = glyph.width.ceil() as u32 + (line_width_offset * 2.0) as u32 + padding * 2;
        let height = glyph.height.ceil() as u32 + (line_width_offset * 2.0) as u32 + padding * 2;

        let (dst_index, dst_image_id, (dst_x, dst_y)) =
            self.find_texture_or_alloc(canvas, width as usize, height as usize)?;

        // render glyph to image
        canvas.save();
        canvas.reset();

        let rendered_bearing_y = glyph.bearing_y.round();
        let x_quant = crate::geometry::quantize(glyph.x.fract(), 0.1);
        let x = dst_x as f32 - glyph.bearing_x + line_width_offset + padding as f32 + x_quant;
        let y = TEXTURE_SIZE as f32 - dst_y as f32 - rendered_bearing_y - line_width_offset - padding as f32;

        let rendered_glyph = RenderedGlyph {
            width: width - 2 * GLYPH_MARGIN,
            height: height - 2 * GLYPH_MARGIN,
            bearing_y: rendered_bearing_y as i32,
            atlas_x: dst_x as u32 + GLYPH_MARGIN,
            atlas_y: dst_y as u32 + GLYPH_MARGIN,
            texture_index: dst_index,
            color_glyph,
        };

        match maybe_glyph_representation.as_mut() {
            Some(GlyphRendering::RenderAsPath(ref mut path)) => {
                canvas.translate(x, y);

                canvas.set_render_target(RenderTarget::Image(dst_image_id));
                canvas.clear_rect(
                    dst_x as u32,
                    TEXTURE_SIZE as u32 - dst_y as u32 - height as u32,
                    width as u32,
                    height as u32,
                    Color::black(),
                );
                let factor = 1.0 / 8.0;

                let mut mask_paint = Paint::color(Color::rgbf(factor, factor, factor));
                mask_paint.set_fill_rule(FillRule::EvenOdd);
                mask_paint.set_anti_alias(false);

                if mode == RenderMode::Stroke {
                    mask_paint.line_width = line_width / scale;
                }

                canvas.global_composite_blend_func(crate::BlendFactor::SrcAlpha, crate::BlendFactor::One);

                // 4x
                // let points = [
                //     (-3.0/8.0, 1.0/8.0),
                //     (1.0/8.0, 3.0/8.0),
                //     (3.0/8.0, -1.0/8.0),
                //     (-1.0/8.0, -3.0/8.0),
                // ];

                // 8x
                let points = [
                    (-7.0 / 16.0, -1.0 / 16.0),
                    (-1.0 / 16.0, -5.0 / 16.0),
                    (3.0 / 16.0, -7.0 / 16.0),
                    (5.0 / 16.0, -3.0 / 16.0),
                    (7.0 / 16.0, 1.0 / 16.0),
                    (1.0 / 16.0, 5.0 / 16.0),
                    (-3.0 / 16.0, 7.0 / 16.0),
                    (-5.0 / 16.0, 3.0 / 16.0),
                ];

                for point in &points {
                    canvas.save();
                    canvas.translate(point.0, point.1);

                    canvas.scale(scale, scale);

                    if mode == RenderMode::Stroke {
                        canvas.stroke_path(path, mask_paint);
                    } else {
                        canvas.fill_path(path, mask_paint);
                    }

                    canvas.restore();
                }
            }
            #[cfg(feature = "image-loading")]
            Some(GlyphRendering::RenderAsImage(image_buffer)) => {
                use std::convert::TryFrom;
                let target_x = rendered_glyph.atlas_x as usize;
                let target_y = rendered_glyph.atlas_y as usize;
                let target_width = rendered_glyph.width as u32;
                let target_height = rendered_glyph.height as u32;

                let image_buffer =
                    image_buffer.resize(target_width, target_height, image::imageops::FilterType::Nearest);
                if let Some(image) = crate::image::ImageSource::try_from(&image_buffer).ok() {
                    canvas.update_image(dst_image_id, image, target_x, target_y).unwrap();
                }
            }
            _ => {}
        }

        canvas.restore();

        Ok(rendered_glyph)
    }

    // Returns (texture index, image id, glyph padding box)
    fn find_texture_or_alloc<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        width: usize,
        height: usize,
    ) -> Result<(usize, ImageId, (usize, usize)), ErrorKind> {
        // Find a free location in one of the atlases
        let mut texture_search_result = {
            let mut glyph_textures = self.glyph_textures.borrow_mut();
            let mut textures = glyph_textures.iter_mut().enumerate();
            textures.find_map(|(index, texture)| {
                texture
                    .atlas
                    .add_rect(width, height)
                    .map(|loc| (index, texture.image_id, loc))
            })
        };

        if texture_search_result.is_none() {
            // All atlases are exausted and a new one must be created
            let mut atlas = Atlas::new(TEXTURE_SIZE, TEXTURE_SIZE);

            let loc = atlas
                .add_rect(width, height)
                .ok_or(ErrorKind::FontSizeTooLargeForAtlas)?;

            // Using PixelFormat::Gray8 works perfectly and takes less VRAM.
            // We keep Rgba8 for now because it might be useful for sub-pixel
            // anti-aliasing (ClearType®), and the atlas debug display is much
            // clearer with different colors. Also, Rgba8 is required for color
            // fonts (typically used for emojis).
            let info = ImageInfo::new(ImageFlags::empty(), atlas.size().0, atlas.size().1, PixelFormat::Rgba8);
            let image_id = canvas.images.alloc(&mut canvas.renderer, info)?;

            #[cfg(feature = "debug_inspector")]
            if cfg!(debug_assertions) {
                // Fill the texture with red pixels only in debug builds.
                if let Ok(size) = canvas.image_size(image_id) {
                    // With image-loading we then subsequently support color fonts, where
                    // the color glyphs are uploaded directly. Since that's immediately and
                    // the clear_rect() is run much later, it would overwrite any uploaded
                    // glyphs. So then when for the debug-inspector, use an image to clear.
                    #[cfg(feature = "image-loading")]
                    {
                        use rgb::FromSlice;
                        let clear_image = image::RgbaImage::from_pixel(
                            size.0 as u32,
                            size.1 as u32,
                            image::Rgba::<u8>([255, 0, 0, 0]),
                        );
                        canvas
                            .update_image(
                                image_id,
                                crate::image::ImageSource::from(imgref::Img::new(
                                    clear_image.as_ref().as_rgba(),
                                    clear_image.width() as usize,
                                    clear_image.height() as usize,
                                )),
                                0,
                                0,
                            )
                            .unwrap();
                    }
                    #[cfg(not(feature = "image-loading"))]
                    {
                        canvas.save();
                        canvas.reset();
                        canvas.set_render_target(RenderTarget::Image(image_id));
                        canvas.clear_rect(
                            0,
                            0,
                            size.0 as u32,
                            size.1 as u32,
                            Color::rgb(255, 0, 0), // Shown as white if using Gray8.,
                        );
                        canvas.restore();
                    }
                }
            }

            self.glyph_textures.borrow_mut().push(FontTexture { atlas, image_id });

            let index = self.glyph_textures.borrow().len() - 1;
            texture_search_result = Some((index, image_id, loc));
        }

        texture_search_result.ok_or(ErrorKind::UnknownError)
    }

    pub(crate) fn clear<T: Renderer>(&self, canvas: &mut Canvas<T>) {
        let image_ids = std::mem::take(&mut *self.glyph_textures.borrow_mut())
            .into_iter()
            .map(|font_texture| font_texture.image_id);
        image_ids.for_each(|id| canvas.delete_image(id));

        self.rendered_glyphs.borrow_mut().clear();
    }
}

pub(crate) fn render_direct<T: Renderer>(
    canvas: &mut Canvas<T>,
    text_layout: &TextMetrics,
    paint: &Paint,
    mode: RenderMode,
    invscale: f32,
) -> Result<(), ErrorKind> {
    let mut paint = *paint;
    paint.set_fill_rule(FillRule::EvenOdd);

    let text_context = canvas.text_context.clone();
    let mut text_context = text_context.borrow_mut();

    let mut scaled = false;

    for glyph in &text_layout.glyphs {
        let (glyph_rendering, scale) = {
            let font = text_context.font_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;

            let scale = font.scale(paint.font_size);

            let glyph_rendering = if let Some(glyph_rendering) =
                font.glyph_rendering_representation(glyph.codepoint as u16, paint.font_size as u16)
            {
                glyph_rendering
            } else {
                continue;
            };

            (glyph_rendering, scale)
        };

        canvas.save();

        if mode == RenderMode::Stroke && !scaled {
            paint.line_width /= scale;
            scaled = true;
        }

        canvas.translate(
            (glyph.x - glyph.bearing_x) * invscale,
            (glyph.y + glyph.bearing_y) * invscale,
        );
        canvas.scale(scale * invscale, -scale * invscale);

        match glyph_rendering {
            GlyphRendering::RenderAsPath(mut path) => {
                if mode == RenderMode::Stroke {
                    canvas.stroke_path(&mut path, paint);
                } else {
                    canvas.fill_path(&mut path, paint);
                }
            }
            #[cfg(feature = "image-loading")]
            GlyphRendering::RenderAsImage(_) => unreachable!(),
        }

        canvas.restore();
    }

    Ok(())
}
