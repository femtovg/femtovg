mod helpers;

use cosmic_text::{Attrs, Buffer, CacheKey, FontSystem, Metrics, Shaping, SubpixelBin, SwashCache};
use femtovg::{
    Atlas, Canvas, Color, DrawCommand, GlyphDrawCommands, ImageFlags, ImageId, ImageSource, Paint, Quad, Renderer,
};
use helpers::WindowSurface;
use std::{collections::HashMap, sync::Arc};
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::Window,
};

use imgref::{Img, ImgRef};
use rgb::RGBA8;
use swash::scale::image::Content;

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

pub struct RenderCache {
    swash_cache: SwashCache,
    rendered_glyphs: HashMap<CacheKey, Option<RenderedGlyph>>,
    glyph_textures: Vec<FontTexture>,
}

impl RenderCache {
    pub(crate) fn new() -> Self {
        Self {
            swash_cache: SwashCache::new(),
            rendered_glyphs: HashMap::default(),
            glyph_textures: Vec::default(),
        }
    }

    pub(crate) fn fill_to_cmds<T: Renderer>(
        &mut self,
        system: &mut FontSystem,
        canvas: &mut Canvas<T>,
        buffer: &Buffer,
        position: (f32, f32),
    ) -> GlyphDrawCommands {
        let mut alpha_cmd_map = HashMap::new();
        let mut color_cmd_map = HashMap::new();

        //let total_height = buffer.layout_runs().len() as i32 * buffer.metrics().line_height;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical_glyph = glyph.physical((0.0, 0.0), 1.0);

                let mut cache_key = physical_glyph.cache_key;

                let position_x = position.0 + cache_key.x_bin.as_float();
                let position_y = position.1 + cache_key.y_bin.as_float();
                //let position_x = position_x - run.line_w * justify.0;
                //let position_y = position_y - total_height as f32 * justify.1;
                let (position_x, subpixel_x) = SubpixelBin::new(position_x);
                let (position_y, subpixel_y) = SubpixelBin::new(position_y);
                cache_key.x_bin = subpixel_x;
                cache_key.y_bin = subpixel_y;
                // perform cache lookup for rendered glyph
                let Some(rendered) = self.rendered_glyphs.entry(cache_key).or_insert_with(|| {
                    // resterize glyph
                    let rendered = self.swash_cache.get_image_uncached(system, cache_key)?;

                    // upload it to the GPU
                    // pick an atlas texture for our glyph
                    let content_w = rendered.placement.width as usize;
                    let content_h = rendered.placement.height as usize;
                    let mut found = None;
                    for (texture_index, glyph_atlas) in self.glyph_textures.iter_mut().enumerate() {
                        if let Some((x, y)) = glyph_atlas.atlas.add_rect(content_w, content_h) {
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
                                ImageFlags::NEAREST,
                            )
                            .unwrap();
                        let texture_index = self.glyph_textures.len();
                        let (x, y) = atlas.add_rect(content_w, content_h).unwrap();
                        self.glyph_textures.push(FontTexture { atlas, image_id });
                        (texture_index, x, y)
                    });

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
                            atlas_alloc_x,
                            atlas_alloc_y,
                        )
                        .unwrap();

                    Some(RenderedGlyph {
                        texture_index,
                        width: rendered.placement.width,
                        height: rendered.placement.height,
                        offset_x: rendered.placement.left,
                        offset_y: rendered.placement.top,
                        atlas_x: atlas_alloc_x as u32,
                        atlas_y: atlas_alloc_y as u32,
                        color_glyph: matches!(rendered.content, Content::Color),
                    })
                }) else {
                    continue;
                };

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

                q.x0 = (position_x + physical_glyph.x + rendered.offset_x) as f32;
                q.y0 = (position_y + physical_glyph.y - rendered.offset_y) as f32 + run.line_y;
                q.x1 = q.x0 + rendered.width as f32;
                q.y1 = q.y0 + rendered.height as f32;

                q.s0 = rendered.atlas_x as f32 * it;
                q.t0 = rendered.atlas_y as f32 * it;
                q.s1 = (rendered.atlas_x + rendered.width) as f32 * it;
                q.t1 = (rendered.atlas_y + rendered.height) as f32 * it;

                cmd.quads.push(q);
            }
        }

        GlyphDrawCommands {
            alpha_glyphs: alpha_cmd_map.into_values().collect(),
            color_glyphs: color_cmd_map.into_values().collect(),
        }
    }
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1000, 600, "Text demo", true);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

fn run<W: WindowSurface>(mut canvas: Canvas<W::Renderer>, el: EventLoop<()>, mut surface: W, window: Arc<Window>) {
    let mut font_system = FontSystem::new();
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(20.0, 25.0));
    let mut cache = RenderCache::new();

    buffer.set_text(&mut font_system, LOREM_TEXT, Attrs::new(), Shaping::Advanced);
    el.run(move |event, event_loop_window_target| {
        event_loop_window_target.set_control_flow(winit::event_loop::ControlFlow::Poll);

        match event {
            Event::LoopExiting => event_loop_window_target.exit(),
            Event::WindowEvent { ref event, .. } => match event {
                #[cfg(not(target_arch = "wasm32"))]
                WindowEvent::Resized(physical_size) => {
                    surface.resize(physical_size.width, physical_size.height);
                }
                WindowEvent::CloseRequested => event_loop_window_target.exit(),
                WindowEvent::RedrawRequested => {
                    let dpi_factor = window.scale_factor() as f32;
                    let size = window.inner_size();
                    canvas.set_size(size.width, size.height, 1.0);
                    canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.9, 0.9, 0.9));

                    buffer.set_metrics(&mut font_system, Metrics::new(20.0 * dpi_factor, 25.0 * dpi_factor));
                    buffer.set_size(&mut font_system, Some(size.width as f32), Some(size.height as f32));
                    let cmds = cache.fill_to_cmds(&mut font_system, &mut canvas, &buffer, (0.0, 0.0));
                    canvas.draw_glyph_commands(cmds, &Paint::color(Color::black()));

                    surface.present(&mut canvas);
                }
                _ => (),
            },
            Event::AboutToWait => window.request_redraw(),

            _ => (),
        }
    })
    .unwrap();
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
