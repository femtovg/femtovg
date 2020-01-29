
use std::io;
use std::f32;
use std::fmt;
use std::path::Path;
use std::ops::Range;
use std::error::Error;
use std::hash::{Hash, Hasher};

use fnv::{FnvHasher, FnvHashMap};
use image::{DynamicImage, GrayImage, Luma, GenericImage};

use super::{ImageId, Renderer, ImageFlags, Paint};

use lru_time_cache::LruCache;

use harfbuzz_rs as hb;
use self::hb::hb as hb_sys;
use self::hb::UnicodeBuffer;

mod freetype;
use self::freetype as ft;

mod atlas;
use atlas::Atlas;

const TEXTURE_SIZE: usize = 512;
const MAX_TEXTURE_SIZE: usize = 4096;
const GLYPH_PADDING: u32 = 2;
const LRU_CACHE_CAPACITY: usize = 100;

type Result<T> = std::result::Result<T, FontCacheError>;

type PostscriptName = String;

// harfbuzz-sys doesn't add this symbol for mac builds.
// And we need it since we're using freetype on OSX.
extern "C" {
    pub fn hb_ft_font_create_referenced(face: ft::ffi::FT_Face) -> *mut hb_sys::hb_font_t;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Baseline {
    /// The text baseline is the top of the em square.
    Top,
    /// The text baseline is the middle of the em square.
    Middle,
    /// The text baseline is the normal alphabetic baseline. Default value.
    Alphabetic,
    // The text baseline is the bottom of the bounding box.
    Bottom
}

impl Default for Baseline {
    fn default() -> Self {
        Self::Alphabetic
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub enum GlyphRenderStyle {
    Fill,
    Stroke {
        line_width: u32
    }
}

impl Default for GlyphRenderStyle {
    fn default() -> Self {
        Self::Fill
    }
}

#[derive(Clone)]
pub struct DrawCmd {
    pub image_id: ImageId,
    pub quads: Vec<Quad>
}

pub struct TextLayout {
    pub bbox: [f32; 4],// TODO: Use the Bounds type here
    pub cmds: Vec<DrawCmd>
}

#[derive(Copy, Clone, Default, Debug)]
pub struct Quad {
    pub x0: f32,
    pub y0: f32,
    pub s0: f32,
    pub t0: f32,
    pub x1: f32,
    pub y1: f32,
    pub s1: f32,
    pub t1: f32
}

#[derive(Hash, Eq, PartialEq)]
struct GlyphId {
    glyph_index: u32,
    size: u32,
    blur: u32,
    render_style: GlyphRenderStyle
}

impl GlyphId {
    pub fn new(index: u32, paint: Paint, render_style: GlyphRenderStyle) -> Self {
        Self {
            glyph_index: index,
            size: paint.font_size(),
            blur: (paint.font_blur() * 1000.0) as u32,
            render_style: render_style,
        }
    }
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct ShapingId {
    text_hash: u64,
    size: u32
}

impl ShapingId {
    pub fn new(size: u32, text: &str) -> Self {
        let mut hasher = FnvHasher::default();
        text.hash(&mut hasher);

        ShapingId {
            text_hash: hasher.finish(),
            size
        }
    }
}

#[derive(Copy, Clone)]
struct Glyph {
    width: u32,
    height: u32,
    atlas_x: u32,
    atlas_y: u32,
    bearing_x: i32,
    bearing_y: i32,
    padding: u32,
    texture_index: usize,
}

struct FontFace {
    id: usize,
    ft_face: ft::Face,
    is_serif: bool,
    is_italic: bool,
    is_bold: bool,
    glyphs: FnvHashMap<GlyphId, Glyph>,
    shaping_cache: LruCache<ShapingId, hb::GlyphBuffer>
}

impl FontFace {
    pub fn new(id: usize, face: ft::Face) -> Self {

        let is_serif = if let Some(ps_name) = face.postscript_name() {
            ps_name.to_lowercase().contains("serif")
        } else {
            false
        };

        let style_flags = face.style_flags();

        Self {
            id: id,
            ft_face: face,
            is_serif: is_serif,
            is_italic: style_flags.contains(ft::StyleFlag::ITALIC),
            is_bold: style_flags.contains(ft::StyleFlag::BOLD),
            glyphs: Default::default(),
            shaping_cache: LruCache::with_capacity(LRU_CACHE_CAPACITY)
        }
    }
}

pub struct FontTexture {
    atlas: Atlas,
    image_id: ImageId
}

pub struct FontCache {
    library: ft::Library,
    stroker: ft::Stroker,
    faces: FnvHashMap<PostscriptName, FontFace>,
    textures: Vec<FontTexture>,
    last_face_id: usize,
}

impl FontCache {

    pub fn new() -> Result<Self> {
        let library = ft::Library::init()?;
        let stroker = library.new_stroker()?;

        Ok(Self {
            library: library,
            stroker: stroker,
            faces: Default::default(),
            textures: Default::default(),
            last_face_id: Default::default()
        })
    }

    pub fn add_font_file<P: AsRef<Path>>(&mut self, file_path: P) -> Result<()> {
        let data = std::fs::read(file_path)?;

        self.add_font_mem(data)
    }

    pub fn add_font_mem(&mut self, data: Vec<u8>) -> Result<()> {
        let face = self.library.new_memory_face(data, 0)?;

        let postscript_name = face.postscript_name().ok_or_else(|| {
            FontCacheError::GeneralError("Cannot read font postscript name".to_string())
        })?;

        self.faces.insert(postscript_name, FontFace::new(self.last_face_id, face));

        self.last_face_id = self.last_face_id.wrapping_add(1);

        Ok(())
    }

    pub fn layout_text<T: Renderer>(&mut self, x: f32, y: f32, renderer: &mut T, paint: Paint, render_style: GlyphRenderStyle,  text: &str) -> Result<TextLayout> {
        let mut cursor_x = x;
        let mut cursor_y = y;

        let mut cmd_map = FnvHashMap::default();

        let mut layout = TextLayout {
            bbox: [0.0, 0.0, 0.0, 0.0],
            cmds: Vec::new()
        };

        let mut max_descender = f32::MIN;
        let mut max_ascender = 0.0f32;

        let faces = Self::face_character_range(&self.faces, text, paint.font_name())?;

        for (face_name, str_range) in faces {
            let face = self.faces.get_mut(&face_name).ok_or(FontCacheError::FontNotFound)?;
            face.ft_face.set_pixel_sizes(0, paint.font_size()).unwrap();

            let output;
            let shaping_id = ShapingId::new(paint.font_size(), &text[str_range.clone()]);

            if let Some(cached_output) = face.shaping_cache.get(&shaping_id) {
                output = cached_output;
            } else {
                // harfbuzz_rs doesn't provide a safe way of creating Face or a Font from a freetype face
                // And I didn't want to read the file a second time and keep it in memory just to give
                // it to harfbuzz_rs here. hb::Owned will free the pointer correctly.
                let hb_font = unsafe {
                    let raw_font = hb_ft_font_create_referenced(face.ft_face.raw_mut());
                    hb::Owned::from_raw(raw_font)
                };

                let buffer = UnicodeBuffer::new().add_str(&text[str_range]);

                face.shaping_cache.insert(shaping_id, hb::shape(&hb_font, buffer, &[]));

                output = face.shaping_cache.get(&shaping_id).unwrap();
            }

            let positions = output.get_glyph_positions();
            let infos = output.get_glyph_infos();

            let size_metrics = face.ft_face.size_metrics().unwrap();
            max_descender = max_descender.max(size_metrics.descender as f32 / 64.0);
            max_ascender = max_ascender.max(size_metrics.ascender as f32 / 64.0);

            // Subpixel positioning / no hinting

            for (position, info) in positions.iter().zip(infos) {
                let gid = info.codepoint;
                //let cluster = info.cluster;
                let x_advance = position.x_advance as f32 / 64.0;
                let y_advance = position.y_advance as f32 / 64.0;
                let x_offset = position.x_offset as f32 / 64.0;
                let y_offset = position.y_offset as f32 / 64.0;

                let glyph_id = GlyphId::new(gid, paint, render_style);

                let glyph = if let Some(glyph) = face.glyphs.get(&glyph_id) {
                    *glyph
                } else {
                    let glyph = Self::glyph(&mut self.textures, &face.ft_face, renderer, &self.stroker, paint, render_style, gid)?;
                    face.glyphs.insert(glyph_id, glyph);
                    glyph
                };

                let xpos = cursor_x + x_offset + glyph.bearing_x as f32 - (glyph.padding / 2) as f32;
                let ypos = cursor_y + y_offset - glyph.bearing_y as f32 - (glyph.padding / 2) as f32;

                if let Some(texture) = self.textures.get(glyph.texture_index) {
                    let image_id = texture.image_id;
                    let size = texture.atlas.size();
                    let itw = 1.0 / size.0 as f32;
                    let ith = 1.0 / size.1 as f32;

                    let cmd = cmd_map.entry(glyph.texture_index).or_insert_with(|| DrawCmd {
                        image_id: image_id,
                        quads: Vec::new()
                    });

                    let mut q = Quad::default();

                    q.x0 = xpos.floor();
                    q.y0 = ypos.floor();
                    q.x1 = xpos.floor() + glyph.width as f32;
                    q.y1 = ypos.floor() + glyph.height as f32;

                    q.s0 = glyph.atlas_x as f32 * itw;
                    q.t0 = glyph.atlas_y as f32 * ith;
                    q.s1 = (glyph.atlas_x + glyph.width) as f32 * itw;
                    q.t1 = (glyph.atlas_y + glyph.height) as f32 * ith;

                    cmd.quads.push(q);
                }

                cursor_x += x_advance + paint.letter_spacing();
                cursor_y += y_advance;
            }
        }

        layout.bbox[0] = x;
        layout.bbox[1] = y - (max_descender + max_ascender);
        layout.bbox[2] = cursor_x;
        layout.bbox[3] = y - max_descender;

        let width = layout.bbox[0] - layout.bbox[2];

        let offset_x = match paint.text_align() {
            Align::Left => 0.0,
            Align::Right => width.floor(),
            Align::Center => (width / 2.0).floor(),
        };

        let offset_y = match paint.text_baseline() {
            Baseline::Top => max_descender + max_ascender,
            Baseline::Middle => -max_descender,
            Baseline::Alphabetic => 0.0,
            Baseline::Bottom => max_descender,
        };

        layout.bbox[0] += offset_x;
        layout.bbox[2] += offset_x;
        layout.bbox[1] += offset_y;
        layout.bbox[3] += offset_y;

        layout.cmds = cmd_map.drain().map(|(_, mut cmd)| {
            cmd.quads.iter_mut().for_each(|quad| {
                quad.x0 += offset_x;
                quad.y0 += offset_y;
                quad.x1 += offset_x;
                quad.y1 += offset_y;
            });

            cmd
        }).collect();

        Ok(layout)
    }

    fn glyph<T: Renderer>(textures: &mut Vec<FontTexture>, ft_face: &ft::Face, renderer: &mut T, stroker: &ft::Stroker, paint: Paint, render_style: GlyphRenderStyle, glyph_index: u32) -> Result<Glyph> {
        let mut padding = GLYPH_PADDING + paint.font_blur().ceil() as u32;

        // Load Freetype glyph slot and fill or stroke

        ft_face.load_glyph(glyph_index, ft::LoadFlag::DEFAULT | ft::LoadFlag::NO_HINTING)?;

        let glyph_slot = ft_face.glyph();
        let mut glyph = glyph_slot.get_glyph()?;

        if let GlyphRenderStyle::Stroke { line_width } = render_style {
            stroker.set(line_width as i64 * 32, ft::StrokerLineCap::Round, ft::StrokerLineJoin::Round, 0);

            glyph = glyph.stroke(stroker)?;

            padding += line_width;
        }

        let bitmap_glyph = glyph.to_bitmap(ft::RenderMode::Normal, None)?;
        let ft_bitmap = bitmap_glyph.bitmap();
        let bitmap_left = bitmap_glyph.left();
        let bitmap_top = bitmap_glyph.top();

        let width = ft_bitmap.width() as u32 + padding * 2;
        let height = ft_bitmap.rows() as u32 + padding * 2;

        // Extract image data from the freetype bitmap and add padding
        let mut glyph_image = GrayImage::new(width, height);

        let mut ft_glyph_offset = 0;

        for y in 0..height {
            for x in 0..width {
                if (x < padding || x >= width - padding) || (y < padding || y >= height - padding) {
                    let pixel = Luma([0]);
                    glyph_image.put_pixel(x as u32, y as u32, pixel);
                } else {
                    let pixel = Luma([ft_bitmap.buffer()[ft_glyph_offset]]);
                    glyph_image.put_pixel(x as u32, y as u32, pixel);
                    ft_glyph_offset += 1;
                }
            }
        }

        if paint.font_blur() > 0.0 {
            glyph_image = image::imageops::blur(&glyph_image, paint.font_blur());
        }

        //glyph_image.save("/home/ptodorov/glyph_test.png");

        // Find a free location in one of the the atlases
        let texture_search_result = textures.iter_mut().enumerate().find_map(|(index, texture)| {
            texture.atlas.add_rect(width as usize, height as usize).map(|loc| (index, loc))
        });

        let (tex_index, (atlas_x, atlas_y)) = if let Some((tex_index, (atlas_x, atlas_y))) = texture_search_result {
            // A location for the new glyph was found in an extisting atlas
            renderer.update_image(textures[tex_index].image_id, &DynamicImage::ImageLuma8(glyph_image), atlas_x as u32, atlas_y as u32);

            (tex_index, (atlas_x, atlas_y))
        } else {
            // All atlases are exausted and a new one must be created
            let mut atlas_size = TEXTURE_SIZE;

            // Try incrementally larger atlasses until a large enough one
            // is found or the MAX_TEXTURE_SIZE limit is reached
            let (atlas, loc) = loop {
                let mut test_atlas = Atlas::new(atlas_size, atlas_size);

                if let Some(loc) = test_atlas.add_rect(width as usize, height as usize) {
                    break (test_atlas, Some(loc));
                }

                if atlas_size >= MAX_TEXTURE_SIZE {
                    break (test_atlas, None);
                }

                atlas_size *= 2;
            };

            let loc = loc.ok_or(FontCacheError::FontSizeTooLargeForAtlas)?;

            let mut image = GrayImage::new(atlas.size().0 as u32, atlas.size().1 as u32);
            image.copy_from(&glyph_image, loc.0 as u32, loc.1 as u32)?;

            let image_res = renderer.create_image(&DynamicImage::ImageLuma8(image), ImageFlags::empty());
            let image_id = image_res.or_else(|e| Err(FontCacheError::GeneralError(format!("{}", e))))?;

            textures.push(FontTexture { atlas, image_id });

            (textures.len() - 1, loc)
        };

        Ok(Glyph {
            width: width,
            height: height,
            atlas_x: atlas_x as u32,
            atlas_y: atlas_y as u32,
            bearing_x: bitmap_left,
            bearing_y: bitmap_top,
            padding: padding,
            texture_index: tex_index,
        })
    }

    fn face_character_range(faces: &FnvHashMap<PostscriptName, FontFace>, text: &str, preferred_face: &str) -> Result<Vec<(PostscriptName, Range<usize>)>> {
        if faces.is_empty() {
            return Err(FontCacheError::NoFontsAdded);
        }

        let mut res = Vec::new();

        let preffered_face = if faces.contains_key(preferred_face) {
            faces.get(preferred_face).unwrap()
        } else {
            faces.values().next().unwrap()
        };

        let mut current_face = preffered_face;
        let mut current_range: Range<usize> = 0..0;

        for (index, c) in text.char_indices() {
            current_range.end = index;

            // Prefer the user provided face
            if current_face.id != preffered_face.id {
                if preffered_face.ft_face.get_char_index(c as usize) != 0 {
                    res.push((current_face.ft_face.postscript_name().unwrap(), current_range.clone()));

                    current_face = preffered_face;
                    current_range = current_range.end..current_range.end;
                }
            } else if current_face.ft_face.get_char_index(c as usize) == 0 {
                // fallback faces
                let compat_face = Self::find_fallback_face(faces, preffered_face, c);

                if let Some(face) = compat_face {
                    res.push((current_face.ft_face.postscript_name().unwrap(), current_range.clone()));

                    current_face = face;
                    current_range = current_range.end..current_range.end;
                }
            }
        }

        current_range.end = text.len();

        res.push((current_face.ft_face.postscript_name().unwrap(), current_range));

        Ok(res)
    }

    fn find_fallback_face<'a>(faces: &'a FnvHashMap<PostscriptName, FontFace>, preffered_face: &'a FontFace, c: char) -> Option<&'a FontFace> {

        let mut face = faces.values().find(|face| {
            face.is_serif == preffered_face.is_serif &&
            face.is_bold == preffered_face.is_bold &&
            face.is_italic == preffered_face.is_italic &&
            face.ft_face.get_char_index(c as usize) != 0
        });

        if face.is_none() {
            face = faces.values().find(|face| {
                face.is_serif == preffered_face.is_serif &&
                face.is_italic == preffered_face.is_italic &&
                face.ft_face.get_char_index(c as usize) != 0
            });
        }

        if face.is_none() {
            face = faces.values().find(|face| {
                face.is_serif == preffered_face.is_serif &&
                face.ft_face.get_char_index(c as usize) != 0
            });
        }

        if face.is_none() {
            face = faces.values().find(|face| {
                face.ft_face.get_char_index(c as usize) != 0
            });
        }

        face
    }
}

#[derive(Debug)]
pub enum FontCacheError {
    GeneralError(String),
    FontNotFound,
    NoFontsAdded,
    FontSizeTooLargeForAtlas,
    IoError(io::Error),
    FreetypeError(ft::Error),
    ImageError(image::ImageError)
}

impl fmt::Display for FontCacheError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "font cache error")
    }
}

impl From<io::Error> for FontCacheError {
    fn from(error: io::Error) -> Self {
        Self::IoError(error)
    }
}

impl From<ft::Error> for FontCacheError {
    fn from(error: ft::Error) -> Self {
        Self::FreetypeError(error)
    }
}

impl From<image::ImageError> for FontCacheError {
    fn from(error: image::ImageError) -> Self {
        Self::ImageError(error)
    }
}

impl Error for FontCacheError {}
