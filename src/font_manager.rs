
use std::io;
use std::fmt;
use std::path::Path;
use std::error::Error;
use std::collections::HashMap;

mod freetype;
use self::freetype as ft;

use harfbuzz_rs as hb;
use self::hb::hb as hb_sys;
use self::hb::{UnicodeBuffer, HarfbuzzObject};

use fnv::FnvHashMap;
use image::{DynamicImage, GrayImage, Luma};

use super::{ImageId, Atlas, Renderer, ImageFlags, renderer::TextureType};

// TODO: Font fallback for missing characters
// TODO: Color fonts
// TODO: Nearest font matching
// TODO: StyledString type like iOS attributed string? or this may be implementen on top of this lib
// TODO: Stroking letters

const TEXTURE_SIZE: u32 = 512;
const GLYPH_PADDING: u32 = 2;

type Result<T> = std::result::Result<T, FontManagerError>;

type PostscriptName = String;

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub enum RenderStyle {
	Fill,
	Stroke {
		line_width: u32
	}
}

impl Default for RenderStyle {
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
	pub bbox: [f32; 4],// TODO: make a specialized "Bounds" type instead of using 4 float array here
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

#[derive(Copy, Clone)]
pub struct FontStyle<'a> {
	font_name: &'a str,
	size: u32,
	blur: f32,
	letter_spacing: f32,
	render_style: RenderStyle
}

impl<'a> FontStyle<'a> {
	pub fn new(name: &'a str) -> Self {
		Self {
			font_name: name,
			size: 16,
			blur: 0.0,
			letter_spacing: 0.0,
			render_style: Default::default()
		}
	}
	
	pub fn set_size(&mut self, size: u32) {
		self.size = size;
	}
	
	pub fn set_blur(&mut self, blur: f32) {
		self.blur = blur;
	}
	
	pub fn set_letter_spacing(&mut self, letter_spacing: f32) {
		self.letter_spacing = letter_spacing;
	}
}

#[derive(Hash, Eq, PartialEq)]
struct GlyphId {
	glyph_index: u32,
	size: u32,
	blur: u32,
	render_style: RenderStyle
}

impl GlyphId {
	pub fn new(index: u32, font_style: FontStyle) -> Self {
		Self {
			glyph_index: index,
			size: font_style.size,
			blur: (font_style.blur * 1000.0) as u32,
			render_style: font_style.render_style,
		}
	}
}

#[derive(Copy, Clone)]
struct Glyph {
	index: u32,
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
	ft_face: ft::Face,
	glyphs: HashMap<GlyphId, Glyph>
}

impl FontFace {
	pub fn new(mut face: ft::Face) -> Self {
		Self {
			ft_face: face,
			glyphs: Default::default()
		}
	}
}

pub struct FontTexture {
	atlas: Atlas,
	image_id: ImageId
}

pub struct FontManager {
	library: ft::Library,
	faces: HashMap<PostscriptName, FontFace>,
	textures: Vec<FontTexture>
}

// Public
impl FontManager {
	
	pub fn new() -> Result<Self> {
		let mut manager = Self {
			library: ft::Library::init()?,
			faces: Default::default(),
			textures: Default::default(),
		};
		
		Ok(manager)
	}
	
	pub fn add_font_file<P: AsRef<Path>>(&mut self, file_path: P) -> Result<()> {
		let data = std::fs::read(file_path)?;
		
		self.add_font_mem(data)
	}
	
	pub fn add_font_mem(&mut self, data: Vec<u8>) -> Result<()> {
		
		let face = self.library.new_memory_face(data, 0)?;
		
		let postscript_name = face.postscript_name().ok_or_else(|| {
			FontManagerError::GeneralError("Cannot read font postscript name".to_string())
		})?;
		
		self.faces.insert(postscript_name, FontFace::new(face));
		
		Ok(())
	}
	
	pub fn layout_text(&mut self, x: f32, y: f32, renderer: &mut Box<dyn Renderer>, style: FontStyle, text: &str) -> Result<TextLayout> {
		let face = self.faces.get_mut(style.font_name).ok_or(FontManagerError::FontNotFound)?;
		
		face.ft_face.set_pixel_sizes(0, style.size).unwrap();
		
		let hb_font = unsafe {
			let raw_font = hb_sys::hb_ft_font_create_referenced(face.ft_face.raw_mut());
			hb_sys::hb_ot_font_set_funcs(raw_font);
			hb::Font::from_raw(raw_font)
		};
		
		let buffer = UnicodeBuffer::new().add_str(text);
		let output = hb::shape(&hb_font, buffer, &[]);
		
		let positions = output.get_glyph_positions();
		let infos = output.get_glyph_infos();
		
		let line_height = (face.ft_face.size_metrics().unwrap().height >> 6) as f32;
		
		let mut layout = TextLayout {
			bbox: [x, y - line_height, x, y],
			cmds: Vec::new()
		};
		
		let itw = 1.0 / TEXTURE_SIZE as f32;
		let ith = 1.0 / TEXTURE_SIZE as f32;
		
		let mut cursor_x = x as i32;
		let mut cursor_y = y as i32;
		
		let mut cmd_map = FnvHashMap::default();
		
		// No subpixel positioning / full hinting
		
		for (position, info) in positions.iter().zip(infos) {
			let gid = info.codepoint;
			let cluster = info.cluster;
			let x_advance = position.x_advance >> 6;
			let y_advance = position.y_advance >> 6;
			let x_offset = position.x_offset >> 6;
			let y_offset = position.y_offset >> 6;
			
			//dbg!(format!("{:?} {:?}", position.x_advance >> 6, position.x_advance / 64.0));
			
			let glyph = Self::glyph_hb(&mut self.textures, face, renderer, style, gid)?;
			
			let xpos = cursor_x + x_offset + glyph.bearing_x - (glyph.padding / 2) as i32;
			let ypos = cursor_y + y_offset - glyph.bearing_y - (glyph.padding / 2) as i32;
			
			let image_id = self.textures[glyph.texture_index].image_id;
			
			let cmd = cmd_map.entry(glyph.texture_index).or_insert_with(|| DrawCmd {
				image_id: image_id,
				quads: Vec::new()
			});
			
			let mut q = Quad::default();
			
			q.x0 = xpos as f32;
			q.y0 = ypos as f32;
			q.x1 = (xpos + glyph.width as i32) as f32;
			q.y1 = (ypos + glyph.height as i32) as f32;

			q.s0 = glyph.atlas_x as f32 * itw;
			q.t0 = glyph.atlas_y as f32 * ith;
			q.s1 = (glyph.atlas_x + glyph.width) as f32 * itw;
			q.t1 = (glyph.atlas_y + glyph.height) as f32 * ith;
			
			cmd.quads.push(q);
			
			cursor_x += x_advance;
			cursor_y += y_advance;
		}
		
		layout.bbox[2] = cursor_x as f32;
		
		layout.cmds = cmd_map.drain().map(|(k, v)| v).collect();
		
		Ok(layout)
	}
	
}

// Private
impl FontManager {
	
	fn glyph_hb(textures: &mut Vec<FontTexture>, face: &mut FontFace, renderer: &mut Box<dyn Renderer>, style: FontStyle, glyph_index: u32) -> Result<Glyph> {
		let glyph_id = GlyphId::new(glyph_index, style);
		
		if let Some(glyph) = face.glyphs.get(&glyph_id) {
			return Ok(*glyph);
		}
		
		let padding = GLYPH_PADDING + style.blur.ceil() as u32;
		
		face.ft_face.load_glyph(glyph_index, ft::LoadFlag::RENDER | ft::LoadFlag::NO_HINTING);
		
		let ft_glyph = face.ft_face.glyph();
		let ft_bitmap = ft_glyph.bitmap();
		
		let width = ft_glyph.bitmap().width() as u32 + padding * 2;
		let height = ft_glyph.bitmap().rows() as u32 + padding * 2;
		
		// Find a free location in one of the the atlases
		let texture_search_result = textures.iter_mut().enumerate().find_map(|(index, texture)| {
			texture.atlas.add_rect(width as usize, height as usize).map(|loc| (index, loc))
		});

		// Or create a new atlas
		let (tex_index, (atlas_x, atlas_y)) = texture_search_result.unwrap_or_else(|| {
			let image_id = renderer.create_texture(TextureType::Alpha, TEXTURE_SIZE, TEXTURE_SIZE, ImageFlags::empty());
			
			let mut atlas = Atlas::new(TEXTURE_SIZE as usize, TEXTURE_SIZE as usize);
			let loc = atlas.add_rect(width as usize, height as usize).unwrap();
			
			textures.push(FontTexture { atlas, image_id });
			
			(textures.len() - 1, loc)
		});
		
		// Extract image data
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
		
		if style.blur > 0.0 {
			glyph_image = image::imageops::blur(&glyph_image, style.blur);
		}
		
		glyph_image.save("/home/ptodorov/glyph_test.png");
		
		// Upload image
		renderer.update_texture(textures[tex_index].image_id, &DynamicImage::ImageLuma8(glyph_image), atlas_x as u32, atlas_y as u32, width, height);
		
		let glyph = Glyph {
			index: glyph_index,
			width: width,
			height: height,
			atlas_x: atlas_x as u32,
			atlas_y: atlas_y as u32,
			bearing_x: ft_glyph.bitmap_left(),
			bearing_y: ft_glyph.bitmap_top(),
			padding: padding,
			texture_index: tex_index,
		};
		
		face.glyphs.insert(glyph_id, glyph);
		
		Ok(glyph)
	}
}

#[derive(Debug)]
pub enum FontManagerError {
	GeneralError(String),
	FontNotFound,
	IoError(io::Error),
	FreetypeError(ft::Error)
}

impl fmt::Display for FontManagerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "font manager error")
    }
}

impl From<io::Error> for FontManagerError {
	fn from(error: io::Error) -> Self {
		Self::IoError(error)
	}
}

impl From<ft::Error> for FontManagerError {
	fn from(error: ft::Error) -> Self {
		Self::FreetypeError(error)
	}
}

impl Error for FontManagerError {}
