mod helpers;

use cosmic_text::{Attrs, Buffer, CacheKey, FontSystem, Metrics, SubpixelBin};
use femtovg::{
    renderer::OpenGl, Atlas, Canvas, Color, DrawCommand, ErrorKind, GlyphDrawCommands, ImageFlags, ImageId,
    ImageSource, Paint, Quad, Renderer,
};
use std::collections::HashMap;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

#[cfg(not(target_arch = "wasm32"))]
use glutin::prelude::*;
use imgref::{Img, ImgRef};
use rgb::RGBA8;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};

#[cfg(target_arch = "wasm32")]
use winit::window::Window;

const GLYPH_PADDING: u32 = 1;
const GLYPH_MARGIN: u32 = 1;
const TEXTURE_SIZE: usize = 512;

pub struct FontTexture {
    atlas: Atlas,
    image_id: ImageId,
}

#[derive(Copy, Clone, Debug)]
pub struct RenderedGlyph {
    texture_index: usize,
    width: u32,
    height: u32,
    offset_x: i32,
    offset_y: i32,
    atlas_x: u32,
    atlas_y: u32,
    color_glyph: bool,
}

#[derive(Default)]
pub struct RenderCache {
    scale_context: ScaleContext,
    rendered_glyphs: HashMap<CacheKey, Option<RenderedGlyph>>,
    glyph_textures: Vec<FontTexture>,
}

impl RenderCache {
    pub(crate) fn fill_to_cmds<T: Renderer>(
        &mut self,
        system: &FontSystem,
        canvas: &mut Canvas<T>,
        buffer: &Buffer<'_>,
        position: (f32, f32),
    ) -> Result<GlyphDrawCommands, ErrorKind> {
        let mut alpha_cmd_map = HashMap::new();
        let mut color_cmd_map = HashMap::new();

        //let total_height = buffer.layout_runs().len() as i32 * buffer.metrics().line_height;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let mut cache_key = glyph.cache_key;
                let position_x = position.0 + cache_key.x_bin.as_float();
                let position_y = position.1 + cache_key.y_bin.as_float();
                //let position_x = position_x - run.line_w * justify.0;
                //let position_y = position_y - total_height as f32 * justify.1;
                let (position_x, subpixel_x) = SubpixelBin::new(position_x);
                let (position_y, subpixel_y) = SubpixelBin::new(position_y);
                cache_key.x_bin = subpixel_x;
                cache_key.y_bin = subpixel_y;
                // perform cache lookup for rendered glyph
                if let Some(rendered) = self.rendered_glyphs.entry(cache_key).or_insert_with(|| {
                    // ...or insert it

                    // do the actual rasterization
                    let font = system
                        .get_font(cache_key.font_id)
                        .expect("Shaped a nonexistent font. What?");
                    let mut scaler = self
                        .scale_context
                        .builder(font.as_swash())
                        .size(cache_key.font_size as f32)
                        .hint(true)
                        .build();
                    let offset = Vector::new(cache_key.x_bin.as_float(), cache_key.y_bin.as_float());
                    let rendered = Render::new(&[
                        Source::ColorOutline(0),
                        Source::ColorBitmap(StrikeWith::BestFit),
                        Source::Outline,
                    ])
                    .format(Format::Alpha)
                    .offset(offset)
                    .render(&mut scaler, cache_key.glyph_id);

                    // upload it to the GPU
                    rendered.map(|rendered| {
                        // pick an atlas texture for our glyph
                        let content_w = rendered.placement.width as usize;
                        let content_h = rendered.placement.height as usize;
                        let alloc_w = rendered.placement.width + (GLYPH_MARGIN + GLYPH_PADDING) * 2;
                        let alloc_h = rendered.placement.height + (GLYPH_MARGIN + GLYPH_PADDING) * 2;
                        let used_w = rendered.placement.width + GLYPH_PADDING * 2;
                        let used_h = rendered.placement.height + GLYPH_PADDING * 2;
                        let mut found = None;
                        for (texture_index, glyph_atlas) in self.glyph_textures.iter_mut().enumerate() {
                            if let Some((x, y)) = glyph_atlas.atlas.add_rect(alloc_w as usize, alloc_h as usize) {
                                found = Some((texture_index, x, y));
                                break;
                            }
                        }
                        let (texture_index, atlas_alloc_x, atlas_alloc_y) = found.unwrap_or_else(|| {
                            // if no atlas could fit the texture, make a new atlas tyvm
                            // TODO error handling
                            let mut atlas = Atlas::new(TEXTURE_SIZE, TEXTURE_SIZE);
                            let image_id = canvas
                                .create_image(
                                    Img::new(
                                        vec![RGBA8::new(0, 0, 0, 0); TEXTURE_SIZE * TEXTURE_SIZE],
                                        TEXTURE_SIZE,
                                        TEXTURE_SIZE,
                                    )
                                    .as_ref(),
                                    ImageFlags::empty(),
                                )
                                .unwrap();
                            let texture_index = self.glyph_textures.len();
                            let (x, y) = atlas.add_rect(alloc_w as usize, alloc_h as usize).unwrap();
                            self.glyph_textures.push(FontTexture { atlas, image_id });
                            (texture_index, x, y)
                        });

                        let atlas_used_x = atlas_alloc_x as u32 + GLYPH_MARGIN;
                        let atlas_used_y = atlas_alloc_y as u32 + GLYPH_MARGIN;
                        let atlas_content_x = atlas_alloc_x as u32 + GLYPH_MARGIN + GLYPH_PADDING;
                        let atlas_content_y = atlas_alloc_y as u32 + GLYPH_MARGIN + GLYPH_PADDING;

                        let mut src_buf = Vec::with_capacity(content_w * content_h);
                        match rendered.content {
                            Content::Mask => {
                                for chunk in rendered.data.chunks_exact(1) {
                                    src_buf.push(RGBA8::new(chunk[0], 0, 0, 0));
                                }
                            }
                            Content::Color => {
                                for chunk in rendered.data.chunks_exact(4) {
                                    src_buf.push(RGBA8::new(chunk[0], chunk[1], chunk[2], chunk[3]));
                                }
                            }
                            Content::SubpixelMask => unreachable!(),
                        }
                        canvas
                            .update_image::<ImageSource>(
                                self.glyph_textures[texture_index].image_id,
                                ImgRef::new(&src_buf, content_w, content_h).into(),
                                atlas_content_x as usize,
                                atlas_content_y as usize,
                            )
                            .unwrap();

                        RenderedGlyph {
                            texture_index,
                            width: used_w,
                            height: used_h,
                            offset_x: rendered.placement.left,
                            offset_y: rendered.placement.top,
                            atlas_x: atlas_used_x,
                            atlas_y: atlas_used_y,
                            color_glyph: matches!(rendered.content, Content::Color),
                        }
                    })
                }) {
                    let cmd_map = if rendered.color_glyph {
                        &mut color_cmd_map
                    } else {
                        &mut alpha_cmd_map
                    };

                    let cmd = cmd_map.entry(rendered.texture_index).or_insert_with(|| DrawCommand {
                        image_id: self.glyph_textures[rendered.texture_index].image_id,
                        quads: Vec::new(),
                    });

                    let mut q = Quad::default();
                    let it = 1.0 / TEXTURE_SIZE as f32;

                    q.x0 = (position_x + glyph.x_int + rendered.offset_x - GLYPH_PADDING as i32) as f32;
                    q.y0 = (position_y + run.line_y + glyph.y_int - rendered.offset_y - GLYPH_PADDING as i32) as f32;
                    q.x1 = q.x0 + rendered.width as f32;
                    q.y1 = q.y0 + rendered.height as f32;

                    q.s0 = rendered.atlas_x as f32 * it;
                    q.t0 = rendered.atlas_y as f32 * it;
                    q.s1 = (rendered.atlas_x + rendered.width) as f32 * it;
                    q.t1 = (rendered.atlas_y + rendered.height) as f32 * it;

                    cmd.quads.push(q);
                }
            }
        }

        Ok(GlyphDrawCommands {
            alpha_glyphs: alpha_cmd_map.into_values().collect(),
            color_glyphs: color_cmd_map.into_values().collect(),
        })
    }
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1000, 600, "Text demo", true);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

lazy_static::lazy_static! {
    static ref FONT_SYSTEM: FontSystem = FontSystem::new();
}

fn run(
    mut canvas: Canvas<OpenGl>,
    el: EventLoop<()>,
    #[cfg(not(target_arch = "wasm32"))] context: glutin::context::PossiblyCurrentContext,
    #[cfg(not(target_arch = "wasm32"))] surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    window: Window,
) {
    let mut buffer = Buffer::new(&FONT_SYSTEM, Metrics::new(20, 25));
    let mut cache = RenderCache::default();
    buffer.set_text(LOREM_TEXT, Attrs::new());
    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::LoopDestroyed => *control_flow = ControlFlow::Exit,
            Event::WindowEvent { ref event, .. } => match event {
                #[cfg(not(target_arch = "wasm32"))]
                WindowEvent::Resized(physical_size) => {
                    surface.resize(
                        &context,
                        physical_size.width.try_into().unwrap(),
                        physical_size.height.try_into().unwrap(),
                    );
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => (),
            },
            Event::MainEventsCleared => window.request_redraw(),
            Event::RedrawRequested(_) => {
                let dpi_factor = window.scale_factor();
                let size = window.inner_size();
                canvas.set_size(size.width, size.height, 1.0);
                canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.9, 0.9, 0.9));

                buffer.set_metrics(Metrics::new((20.0 * dpi_factor) as i32, (25.0 * dpi_factor) as i32));
                buffer.set_size(size.width as i32, size.height as i32);
                let cmds = cache
                    .fill_to_cmds(&FONT_SYSTEM, &mut canvas, &buffer, (0.0, 0.0))
                    .unwrap();
                canvas.draw_glyph_commands(cmds, &Paint::color(Color::black()), 1.0);

                canvas.flush();
                #[cfg(not(target_arch = "wasm32"))]
                surface.swap_buffers(&context).unwrap();
            }
            _ => (),
        }
    });
}

const LOREM_TEXT: &str = r"
Traditionally, text is composed to create a readable, coherent, and visually satisfying typeface
that works invisibly, without the awareness of the reader. Even distribution of typeset material,
with a minimum of distractions and anomalies, is aimed at producing clarity and transparency.
Choice of typeface(s) is the primary aspect of text typography—prose fiction, non-fiction,
editorial, educational, religious, scientific, spiritual, and commercial writing all have differing
characteristics and requirements of appropriate typefaces and their fonts or styles.

مرئية وساهلة قراءة وجاذبة. ترتيب الحوف يشمل كل من اختيار عائلة الخط وحجم وطول الخط والمسافة بين السطور

مرئية وساهلة قراءة وجاذبة. ترتيب الحوف يشمل كل من اختيار (asdasdasdasdasdasd) عائلة الخط وحجم وطول الخط والمسافة بين السطور

Lorem ipsum dolor sit amet, consectetur adipiscing elit. Curabitur in nisi at ligula lobortis pretium. Sed vel eros tincidunt, fermentum metus sit amet, accumsan massa. Vestibulum sed elit et purus suscipit
Sed at gravida lectus. Duis eu nisl non sem lobortis rutrum. Sed non mauris urna. Pellentesque suscipit nec odio eu varius. Quisque lobortis elit in finibus vulputate. Mauris quis gravida libero.
Etiam non malesuada felis, nec fringilla quam.

😂🤩🥰😊😄
";
