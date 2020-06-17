
use rgb::alt::Gray;
use imgref::ImgVec;

use fnv::FnvHashMap;

use crate::{
    Canvas,
    Renderer,
    Path,
    Paint,
    ErrorKind,
    FillRule,
    geometry::Transform2D,
    ImageId,
    ImageFlags,
    ImageStore,
    ImageFormat,
    ImageInfo,
    RenderTarget,
    Color
};

use super::{
    TextLayout,
    TextStyle,
    RenderStyle,
    Atlas,
    FontDb,
    FontId,
    ShapedGlyph,
    freetype as ft,
    GLYPH_PADDING
};

const TEXTURE_SIZE: usize = 512;
const MAX_TEXTURE_SIZE: usize = 4096;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct RenderedGlyphId {
    glyph_index: u32,
    font_id: FontId,
    size: u16,
    blur: u8,
    render_style: RenderStyle
}

impl RenderedGlyphId {
    fn new(glyph_index: u32, font_id: FontId, style: &TextStyle<'_>) -> Self {
        RenderedGlyphId {
            glyph_index,
            font_id,
            size: style.size,
            blur: style.blur,
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
pub struct TextHelperContext {
    textures: Vec<FontTexture>,
    glyph_cache: FnvHashMap<RenderedGlyphId, RenderedGlyph>
}

pub fn render_text<T: Renderer>(canvas: &mut Canvas<T>, text_layout: &TextLayout, style: &TextStyle<'_>, paint: &Paint, invscale: f32) -> Result<Vec<DrawCmd>, ErrorKind> {
    let mut cmd_map = FnvHashMap::default();

    let initial_render_target = canvas.current_render_target;

    for glyph in &text_layout.glyphs {
        let id = RenderedGlyphId::new(glyph.codepoint, glyph.font_id, style);

        if !canvas.text_helper_context.glyph_cache.contains_key(&id) {
            let glyph = render_glyph(canvas, style, &glyph, paint, invscale)?;

            canvas.text_helper_context.glyph_cache.insert(id.clone(), glyph);
        }

        let rendered = canvas.text_helper_context.glyph_cache.get(&id).unwrap();

        if let Some(texture) = canvas.text_helper_context.textures.get(rendered.texture_index) {
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

    canvas.set_render_target(initial_render_target);

    // debug draw
    {
        canvas.save();
        canvas.reset();

        let image_id = canvas.text_helper_context.textures[0].image_id;

        let mut path = Path::new();
        path.rect(400.0, 20.0, 512.0, 512.0);
        canvas.fill_path(&mut path, Paint::image(image_id, 400.0, 20.0, 512.0, 512.0, 0.0, 1.0));
        canvas.stroke_path(&mut path, Paint::color(Color::black()));

        canvas.restore();
    }

    Ok(cmd_map.drain().map(|(_, cmd)| cmd).collect())
}

pub fn render_glyph<T: Renderer>(
    canvas: &mut Canvas<T>,
    style: &TextStyle<'_>,
    glyph: &ShapedGlyph,
    paint: &Paint,
    invscale: f32
) -> Result<RenderedGlyph, ErrorKind> {
    let mut padding = GLYPH_PADDING + style.blur as u32 * 2;

    // let mut path = {
    //     let font = canvas.fontdb.get_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
    //     let font = ttf_parser::Font::from_data(&font.data, 0).ok_or(ErrorKind::FontParseError)?;
    //     glyph_path(font, glyph.codepoint as u16, style.size as f32, 0.0, 0.0)?
    // };

    // let bounds = canvas.path_bbox(&mut path);
    //let width = (bounds.maxx - bounds.minx) as u32 + padding * 2;
    //let height = (bounds.maxy - bounds.miny) as u32 + padding * 2;

    let width = glyph.width as u32 + padding * 2;
    let height = glyph.height as u32 + padding * 2;

    // Find a free location in one of the the atlases
    let mut texture_search_result = canvas.text_helper_context.textures.iter_mut().enumerate().find_map(|(index, texture)| {
        texture.atlas.add_rect(width as usize, height as usize).map(|loc| (index, texture.image_id, loc))
    });

    if texture_search_result.is_none() {
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

        let info = ImageInfo::new(ImageFlags::PREMULTIPLIED, atlas.size().0, atlas.size().1, ImageFormat::Gray8);
        let image_id = canvas.images.alloc(&mut canvas.renderer, info)?;

        //let image = ImgVec::new(vec![Gray(0u8); atlas.size().0 * atlas.size().1], atlas.size().0, atlas.size().1);
        //canvas.images.update(&mut canvas.renderer, image_id, image.as_ref().into(), 0, 0)?;

        // canvas.save();
        // canvas.reset();
        // canvas.set_render_target(RenderTarget::Image(image_id));
        // canvas.clear_rect(0, 0, atlas_size as u32, atlas_size as u32, Color::black());
        //let mut path = Path::new();
        //path.rect(0.0, 0.0, atlas_size as f32, atlas_size as f32);
        //canvas.fill_path(&mut path, Paint::color(Color::black()));
        //canvas.restore();
        
        canvas.text_helper_context.textures.push(FontTexture { atlas, image_id });

        let index = canvas.text_helper_context.textures.len() - 1;
        texture_search_result = Some((index, image_id, loc));
    }

    let (index, image_id, (x, y)) = texture_search_result.unwrap();

    // render glyph to image
    canvas.save();
    canvas.reset();
    canvas.set_render_target(RenderTarget::Image(image_id));

    // TODO: Find a way to remove this dupe call of glyph_path
    let mut path = {
        let font = canvas.fontdb.get_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
        let font = ttf_parser::Font::from_data(&font.data, 0).ok_or(ErrorKind::FontParseError)?;
        glyph_path(font, glyph.codepoint as u16, style.size as f32, x as f32, 512.0 - y as f32 - glyph.bearing_y as f32)?
    };

    //canvas.clear_rect(0, 0, 512, 512, Color::white());

    // let mut square_path = Path::new();
    // square_path.rect(x as f32, 512.0 - glyph.height - y as f32, glyph.width, glyph.height);
    // canvas.fill_path(&mut square_path, Paint::color(Color::white()));

    canvas.clear_rect(x as u32, 512 - y as u32 - height as u32, width as u32, height as u32, Color::black());

    let mut paint = Paint::color(Color::white());
    paint.set_fill_rule(FillRule::EvenOdd);
    paint.set_anti_alias(false);

    canvas.fill_path(&mut path, paint);

    canvas.restore();

    if style.blur > 0 {
        // canvas.renderer.blur(
        //     canvas.images.get_mut(image_id).unwrap(),
        //     style.blur,
        //     x + style.blur as usize,
        //     y + style.blur as usize,
        //     width as usize - style.blur as usize,
        //     height as usize - style.blur as usize,
        // );
    }

    Ok(RenderedGlyph {
        width: width,
        height: height,
        atlas_x: x as u32,
        atlas_y: y as u32,
        texture_index: index,
        padding: padding,
    })
}

// TODO this uses the canvas to draw glyphs directly on the screen. This is only OK for large glyph sizes
pub fn render_text_direct<T: Renderer>(canvas: &mut Canvas<T>, text_layout: &TextLayout, style: &TextStyle<'_>, paint: &Paint, invscale: f32) -> Result<(), ErrorKind> {
    let mut paint = *paint;
    paint.set_fill_rule(FillRule::EvenOdd);
    paint.set_anti_alias(false);

    for glyph in &text_layout.glyphs {
        let mut path = {
            let font = canvas.fontdb.get_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
            let font = ttf_parser::Font::from_data(&font.data, 0).ok_or(ErrorKind::FontParseError)?;

            glyph_path(font, glyph.codepoint as u16, style.size as f32, glyph.x * invscale, glyph.y * invscale)?

            // let units_per_em = font.units_per_em().ok_or(ErrorKind::FontParseError)?;
            // let scale = paint.font_size() as f32 / units_per_em as f32;

            // let mut transform = Transform2D::identity();
            // transform.scale(scale, -scale);
            // transform.translate(glyph.x * invscale, glyph.y * invscale);

            // let mut path_builder = TransformedPathBuilder(Path::new(), transform);
            // font.outline_glyph(ttf_parser::GlyphId(glyph.codepoint as u16), &mut path_builder);

            // path_builder.0
        };

        if let RenderStyle::Stroke { width } = style.render_style {
            paint.set_stroke_width(width as f32);
            canvas.stroke_path(&mut path, paint);
        } else {
            canvas.fill_path(&mut path, paint);
        }
    }

    Ok(())
}

fn glyph_path(font: ttf_parser::Font, codepoint: u16, size: f32, x: f32, y: f32) -> Result<Path, ErrorKind> {
    let units_per_em = font.units_per_em().ok_or(ErrorKind::FontParseError)?;
    let scale = size / units_per_em as f32;

    let mut transform = Transform2D::identity();

    transform.scale(scale, scale);
    transform.translate(x, y);
    
    let mut path_builder = TransformedPathBuilder(Path::new(), transform);
    font.outline_glyph(ttf_parser::GlyphId(codepoint as u16), &mut path_builder);

    Ok(path_builder.0)
}

struct TransformedPathBuilder(Path, Transform2D);

impl ttf_parser::OutlineBuilder for TransformedPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        self.0.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        self.0.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        let (x1, y1) = self.1.transform_point(x1, y1);
        self.0.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x, y) = self.1.transform_point(x, y);
        let (x1, y1) = self.1.transform_point(x1, y1);
        let (x2, y2) = self.1.transform_point(x2, y2);
        self.0.bezier_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.0.close();
    }
}