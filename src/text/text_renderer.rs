
use rgb::alt::Gray;
use imgref::ImgVec;

use fnv::FnvHashMap;

use crate::{
    Renderer,
    ImageId,
    ImageFlags,
    ImageStore,
    ErrorKind
};

use super::{
    FontDb,
    FontId,
    TextStyle,
    RenderStyle,
    TextLayout,
    ShapedGlyph,
    freetype as ft,
    GLYPH_PADDING
};

mod atlas;
use atlas::Atlas;

const TEXTURE_SIZE: usize = 512;
const MAX_TEXTURE_SIZE: usize = 4096;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct RenderedGlyphId {
    glyph_index: u32,
    font_id: FontId,
    size: u16,
    blur: u32,
    render_style: RenderStyle
}

impl RenderedGlyphId {
    fn new(glyph_index: u32, font_id: FontId, style: &TextStyle<'_>) -> Self {
        RenderedGlyphId {
            glyph_index,
            font_id,
            size: style.size,
            blur: (style.blur * 1000.0) as u32,
            render_style: style.render_style
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct RenderedGlyph {
    texture_index: usize,
    width: u32,
    height: u32,
    atlas_x: u32,
    atlas_y: u32,
    padding: u32,
}

#[derive(Clone)]
pub struct DrawCmd {
    pub image_id: ImageId,
    pub quads: Vec<Quad>
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

pub struct FontTexture {
    atlas: Atlas,
    image_id: ImageId
}

#[derive(Default)]
pub struct TextRenderer {
    textures: Vec<FontTexture>,
    glyph_cache: FnvHashMap<RenderedGlyphId, RenderedGlyph>
}

impl TextRenderer {

    pub(crate) fn render<T: Renderer>(
        &mut self,
        renderer: &mut T,
        images: &mut ImageStore<T::Image>,
        fontdb: &mut FontDb,
        text_layout: &TextLayout,
        style: &TextStyle<'_>
    ) -> Result<Vec<DrawCmd>, ErrorKind> {

        let mut cmd_map = FnvHashMap::default();

        let textures = &mut self.textures;

        for glyph in &text_layout.glyphs {
            let id = RenderedGlyphId::new(glyph.codepoint, glyph.font_id, style);

            if !self.glyph_cache.contains_key(&id) {
                let glyph = Self::render_glyph(renderer, images, textures, fontdb, style, &glyph)?;
                self.glyph_cache.insert(id.clone(), glyph);
            }

            let rendered = self.glyph_cache.get(&id).unwrap();

            if let Some(texture) = textures.get(rendered.texture_index) {
                let image_id = texture.image_id;
                let size = texture.atlas.size();
                let itw = 1.0 / size.0 as f32;
                let ith = 1.0 / size.1 as f32;

                let cmd = cmd_map.entry(rendered.texture_index).or_insert_with(|| DrawCmd {
                    image_id: image_id,
                    quads: Vec::new()
                });

                let mut q = Quad::default();

                q.x0 = glyph.x;
                q.y0 = glyph.y;
                q.x1 = glyph.x + rendered.width as f32;
                q.y1 = glyph.y + rendered.height as f32;

                q.s0 = rendered.atlas_x as f32 * itw;
                q.t0 = rendered.atlas_y as f32 * ith;
                q.s1 = (rendered.atlas_x + rendered.width) as f32 * itw;
                q.t1 = (rendered.atlas_y + rendered.height) as f32 * ith;

                cmd.quads.push(q);
            }
        }

        Ok(cmd_map.drain().map(|(_, cmd)| cmd).collect())
    }

    fn render_glyph<T: Renderer>(
        renderer: &mut T,
        images: &mut ImageStore<T::Image>,
        textures: &mut Vec<FontTexture>,
        fontdb: &mut FontDb,
        style: &TextStyle<'_>,
        glyph: &ShapedGlyph
    ) -> Result<RenderedGlyph, ErrorKind> {
        // TODO: Review data types to reduce "var as type"

        let mut padding = GLYPH_PADDING + style.blur.ceil() as u32;

        let stroker = fontdb.library.new_stroker()?;

        let font = fontdb.get_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;

        // Load Freetype glyph slot and fill or stroke
        //let index = font.face.get_char_index(glyph.codepoint as u32);
        font.face.load_glyph(glyph.codepoint, ft::LoadFlag::DEFAULT)?;

        let glyph_slot = font.face.glyph();
        let mut glyph = glyph_slot.get_glyph()?;

        if let RenderStyle::Stroke { width } = style.render_style {
            stroker.set(width as i64 * 32, ft::StrokerLineCap::Round, ft::StrokerLineJoin::Round, 0);

            glyph = glyph.stroke(&stroker)?;

            padding += width as u32;
        }

        let bitmap_glyph = glyph.to_bitmap(ft::RenderMode::Normal, None)?;
        let ft_bitmap = bitmap_glyph.bitmap();
        //let bitmap_left = bitmap_glyph.left();
        //let bitmap_top = bitmap_glyph.top();

        let width = ft_bitmap.width() as u32 + padding * 2;
        let height = ft_bitmap.rows() as u32 + padding * 2;

        // Extract image data from the freetype bitmap and add padding
        let mut glyph_image = ImgVec::new(vec![Gray(0u8); width as usize * height as usize], width as usize, height as usize);

        let mut ft_glyph_offset = 0;

        for y in 0..height {
            for x in 0..width {
                if (x < padding || x >= width - padding) || (y < padding || y >= height - padding) {

                } else {
                    glyph_image[(x, y)] = Gray(ft_bitmap.buffer()[ft_glyph_offset]);
                    ft_glyph_offset += 1;
                }
            }
        }

        if style.blur > 0.0 {
            // TODO: Do renderer blurring
            //glyph_image = image::imageops::blur(&glyph_image, style.blur);
        }

        //glyph_image.save("/home/ptodorov/glyph_test.png");

        // Find a free location in one of the the atlases
        let texture_search_result = textures.iter_mut().enumerate().find_map(|(index, texture)| {
            texture.atlas.add_rect(width as usize, height as usize).map(|loc| (index, loc))
        });

        let (tex_index, (atlas_x, atlas_y)) = if let Some((tex_index, (atlas_x, atlas_y))) = texture_search_result {
            // A location for the new glyph was found in an extisting atlas
            images.update(renderer, textures[tex_index].image_id, glyph_image.as_ref().into(), atlas_x, atlas_y)?;

            if style.blur > 0.0 {
                renderer.blur(images.get_mut(textures[tex_index].image_id).unwrap(), style.blur, atlas_x, atlas_y, width as usize, height as usize);
            }

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

            let loc = loc.ok_or(ErrorKind::FontSizeTooLargeForAtlas)?;

            let mut image = ImgVec::new(vec![Gray(0u8); atlas.size().0 * atlas.size().1], atlas.size().0, atlas.size().1);

            // Copy glyph image to atlas image
            for (y_offset, row) in glyph_image.rows().enumerate() {
                for (x_offset, element) in row.iter().enumerate() {
                    image[(loc.0 + x_offset, loc.1 + y_offset)] = *element;
                }
            }

            let image_id = images.add(renderer, image.as_ref().into(), ImageFlags::empty())?;

            if style.blur > 0.0 {
                renderer.blur(images.get_mut(image_id).unwrap(), style.blur, loc.0, loc.1, width as usize, height as usize);
            }

            textures.push(FontTexture { atlas, image_id });

            (textures.len() - 1, loc)
        };

        Ok(RenderedGlyph {
            width: width,
            height: height,
            atlas_x: atlas_x as u32,
            atlas_y: atlas_y as u32,
            texture_index: tex_index,
            padding: padding,
        })
    }
}
