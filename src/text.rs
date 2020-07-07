use std::ffi::OsStr;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path as FilePath;

use fnv::{FnvBuildHasher, FnvHashMap, FnvHasher};
use generational_arena::{Arena, Index};
use harfbuzz_rs as hb;
use lru::LruCache;

use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Canvas, Color, ErrorKind, FillRule, ImageFlags, ImageId, ImageInfo, ImageStore, Paint, Path, PixelFormat,
    RenderTarget, Renderer,
};

mod atlas;
pub use atlas::Atlas;

mod font;
use font::Font;
pub use font::FontMetrics;

const GLYPH_PADDING: u32 = 2;
const TEXTURE_SIZE: usize = 512;
const LRU_CACHE_CAPACITY: usize = 1000;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
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

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
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
struct RenderedGlyphId {
    glyph_index: u32,
    font_id: FontId,
    size: u32,
    line_width: u32,
    render_mode: RenderMode,
}

impl RenderedGlyphId {
    fn new(glyph_index: u32, font_id: FontId, paint: &Paint, mode: RenderMode) -> Self {
        RenderedGlyphId {
            glyph_index,
            font_id,
            size: (paint.font_size * 10.0).trunc() as u32,
            line_width: (paint.line_width * 10.0).trunc() as u32,
            render_mode: mode,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct RenderedGlyph {
    texture_index: usize,
    width: u32,
    height: u32,
    atlas_x: u32,
    atlas_y: u32,
    padding: u32,
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
    fn new(paint: &Paint, word: &str) -> Self {
        let mut hasher = FnvHasher::default();
        word.hash(&mut hasher);

        ShapingId {
            size: (paint.font_size * 10.0).trunc() as u32,
            word_hash: hasher.finish(),
            font_ids: paint.font_ids,
        }
    }
}

type ShapedWordsCache<H> = LruCache<ShapingId, Result<ShapedWord, ErrorKind>, H>;
type ShapingRunCache<H> = LruCache<ShapingId, TextMetrics, H>;

struct FontTexture {
    atlas: Atlas,
    image_id: ImageId,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FontId(Index);

pub(crate) struct TextContext {
    fonts: Arena<Font>,
    shaping_run_cache: ShapingRunCache<FnvBuildHasher>,
    shaped_words_cache: ShapedWordsCache<FnvBuildHasher>,
    textures: Vec<FontTexture>,
    rendered_glyphs: FnvHashMap<RenderedGlyphId, RenderedGlyph>,
}

impl Default for TextContext {
    fn default() -> Self {
        let fnv_run = FnvBuildHasher::default();
        let fnv_words = FnvBuildHasher::default();

        Self {
            fonts: Default::default(),
            shaping_run_cache: LruCache::with_hasher(LRU_CACHE_CAPACITY, fnv_run),
            shaped_words_cache: LruCache::with_hasher(LRU_CACHE_CAPACITY, fnv_words),
            textures: Default::default(),
            rendered_glyphs: Default::default(),
        }
    }
}

impl TextContext {
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

    pub fn add_font_mem(&mut self, data: &[u8]) -> Result<FontId, ErrorKind> {
        self.clear_caches();

        let font = Font::new(data)?;
        Ok(FontId(self.fonts.insert(font)))
    }

    pub fn font(&self, id: FontId) -> Option<&Font> {
        self.fonts.get(id.0)
    }

    pub fn font_mut(&mut self, id: FontId) -> Option<&mut Font> {
        self.fonts.get_mut(id.0)
    }

    pub fn find_font<F, T>(&mut self, _text: &str, paint: &Paint, mut callback: F) -> Result<T, ErrorKind>
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
}

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
}

// Shaper

pub(crate) fn shape(
    x: f32,
    y: f32,
    context: &mut TextContext,
    paint: &Paint,
    text: &str,
    max_width: Option<f32>,
) -> Result<TextMetrics, ErrorKind> {
    let id = ShapingId::new(paint, text);

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
    context: &mut TextContext,
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
    hb_direction: hb::Direction,
    context: &mut TextContext,
    paint: &Paint,
) -> Result<ShapedWord, ErrorKind> {
    // find_font will call the closure with each font matching the provided style
    // until a font capable of shaping the word is found
    context.find_font(&word, paint, |(font_id, font)| {
        // Call harfbuzz
        let output = {
            // TODO: It may be faster if this is created only once and stored inside the Font struct
            let face = hb::Face::new(font.data().clone(), 0);
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
fn layout(x: f32, y: f32, context: &mut TextContext, res: &mut TextMetrics, paint: &Paint) -> Result<(), ErrorKind> {
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
        let font = context.font_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;

        // Baseline alignment
        let metrics = font.metrics(paint.font_size);

        let alignment_offset_y = match paint.text_baseline {
            Baseline::Top => metrics.ascender(),
            Baseline::Middle => (metrics.ascender() + metrics.descender()) / 2.0,
            Baseline::Alphabetic => 0.0,
            Baseline::Bottom => metrics.descender(),
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

// Renderer

#[derive(Clone)]
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

pub(crate) fn render_atlas<T: Renderer>(
    canvas: &mut Canvas<T>,
    text_layout: &TextMetrics,
    paint: &Paint,
    mode: RenderMode,
) -> Result<Vec<DrawCmd>, ErrorKind> {
    let mut cmd_map = FnvHashMap::default();

    let half_line_width = if mode == RenderMode::Stroke {
        paint.line_width / 2.0
    } else {
        0.0
    };

    let initial_render_target = canvas.current_render_target;

    for glyph in &text_layout.glyphs {
        let id = RenderedGlyphId::new(glyph.codepoint, glyph.font_id, paint, mode);

        if !canvas.text_context.rendered_glyphs.contains_key(&id) {
            let glyph = render_glyph(canvas, paint, mode, &glyph)?;

            canvas.text_context.rendered_glyphs.insert(id, glyph);
        }

        let rendered = canvas.text_context.rendered_glyphs.get(&id).unwrap();

        if let Some(texture) = canvas.text_context.textures.get(rendered.texture_index) {
            let image_id = texture.image_id;
            let size = texture.atlas.size();
            let itw = 1.0 / size.0 as f32;
            let ith = 1.0 / size.1 as f32;

            let cmd = cmd_map.entry(rendered.texture_index).or_insert_with(|| DrawCmd {
                image_id,
                quads: Vec::new(),
            });

            let mut q = Quad::default();

            q.x0 = glyph.x - half_line_width - GLYPH_PADDING as f32;
            q.y0 = glyph.y - half_line_width - GLYPH_PADDING as f32;
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

    // debug draw
    // {
    //     canvas.save();
    //     canvas.reset();

    //     let image_id = canvas.text_renderer_context.textures[0].image_id;

    //     let mut path = Path::new();
    //     path.rect(20.5, 20.5, 512.0, 512.0);
    //     canvas.fill_path(&mut path, Paint::image(image_id, 20.5, 20.5, 512.0, 512.0, 0.0, 1.0));
    //     canvas.stroke_path(&mut path, Paint::color(Color::black()));

    //     canvas.restore();
    // }

    Ok(cmd_map.drain().map(|(_, cmd)| cmd).collect())
}

fn render_glyph<T: Renderer>(
    canvas: &mut Canvas<T>,
    paint: &Paint,
    mode: RenderMode,
    glyph: &ShapedGlyph
) -> Result<RenderedGlyph, ErrorKind> {
    // TODO: this may be blur * 2 - fix it when blurring iss implemented
    let padding = GLYPH_PADDING;

    let line_width = if mode == RenderMode::Stroke {
        paint.line_width
    } else {
        0.0
    };

    let width = glyph.width.ceil() as u32 + line_width.ceil() as u32 + padding * 2;
    let height = glyph.height.ceil() as u32 + line_width.ceil() as u32 + padding * 2;

    let (dst_index, dst_image_id, (dst_x, dst_y)) = find_texture_or_alloc(
        &mut canvas.text_context.textures,
        &mut canvas.images,
        &mut canvas.renderer,
        width as usize,
        height as usize,
    )?;

    // render glyph to image
    canvas.save();
    canvas.reset();

    let (mut path, scale) = {
        let font = canvas
            .text_context
            .font_mut(glyph.font_id)
            .ok_or(ErrorKind::NoFontFound)?;
        let scale = font.scale(paint.font_size);

        let path = if let Some(font_glyph) = font.glyph(glyph.codepoint as u16) {
            font_glyph.path.clone()
        } else {
            Path::new()
        };

        (path, scale)
    };

    let x = dst_x as f32 - glyph.bearing_x + (line_width / 2.0) + padding as f32;
    let y = TEXTURE_SIZE as f32 - dst_y as f32 - glyph.bearing_y - (line_width / 2.0) - padding as f32;

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
        (5.0 / 16.0, 1.0 / 16.0),
        (1.0 / 16.0, 5.0 / 16.0),
        (-3.0 / 16.0, 7.0 / 16.0),
        (-5.0 / 16.0, 3.0 / 16.0),
    ];

    for point in &points {
        canvas.save();
        canvas.translate(point.0, point.1);

        canvas.scale(scale, scale);

        if mode == RenderMode::Stroke {
            canvas.stroke_path(&mut path, mask_paint);
        } else {
            canvas.fill_path(&mut path, mask_paint);
        }

        canvas.restore();
    }

    canvas.restore();

    Ok(RenderedGlyph {
        width: width,
        height: height,
        atlas_x: dst_x as u32,
        atlas_y: dst_y as u32,
        texture_index: dst_index,
        padding: padding,
    })
}

fn find_texture_or_alloc<T: Renderer>(
    textures: &mut Vec<FontTexture>,
    images: &mut ImageStore<T::Image>,
    renderer: &mut T,
    width: usize,
    height: usize,
) -> Result<(usize, ImageId, (usize, usize)), ErrorKind> {
    // Find a free location in one of the the atlases
    let mut texture_search_result = textures.iter_mut().enumerate().find_map(|(index, texture)| {
        texture
            .atlas
            .add_rect(width, height)
            .map(|loc| (index, texture.image_id, loc))
    });

    if texture_search_result.is_none() {
        // All atlases are exausted and a new one must be created
        let mut atlas = Atlas::new(TEXTURE_SIZE, TEXTURE_SIZE);

        let loc = atlas.add_rect(width, height).ok_or(ErrorKind::FontSizeTooLargeForAtlas)?;

        let info = ImageInfo::new(ImageFlags::empty(), atlas.size().0, atlas.size().1, PixelFormat::Gray8);
        let image_id = images.alloc(renderer, info)?;

        textures.push(FontTexture { atlas, image_id });

        let index = textures.len() - 1;
        texture_search_result = Some((index, image_id, loc));
    }

    texture_search_result.ok_or(ErrorKind::UnknownError)
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

    let mut scaled = false;

    for glyph in &text_layout.glyphs {
        let (mut path, scale) = {
            let font = canvas
                .text_context
                .font_mut(glyph.font_id)
                .ok_or(ErrorKind::NoFontFound)?;
            let scale = font.scale(paint.font_size);

            let path = if let Some(font_glyph) = font.glyph(glyph.codepoint as u16) {
                font_glyph.path.clone()
            } else {
                continue;
            };

            (path, scale)
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

        if mode == RenderMode::Stroke {
            canvas.stroke_path(&mut path, paint);
        } else {
            canvas.fill_path(&mut path, paint);
        }

        canvas.restore();
    }

    Ok(())
}
