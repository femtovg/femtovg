// Copyright 2024 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Original example by Parley Authors, modified for femtovg.
//! You can find the original example source code at
//! https://github.com/linebender/parley/blob/7b9a6f938068d37a3e4218a048cda920803c1f89/examples/swash_render/src/main.rs

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::shadow_unrelated)]

mod helpers;

use femtovg::{
    Atlas, Canvas, Color, DrawCommand, GlyphDrawCommands, ImageFlags, ImageId, ImageSource, Paint, Path, Quad, Renderer,
};
use helpers::WindowSurface;
use imgref::{Img, ImgRef};
use parley::{
    layout::{Alignment, Glyph, GlyphRun, Layout, PositionedLayoutItem},
    style::{FontStack, StyleProperty},
    FontContext, LayoutContext,
};
use rgb::RGBA8;
use std::{collections::HashMap, sync::Arc};
use swash::{
    scale::{image::Content, Render, ScaleContext, Scaler, Source, StrikeWith},
    zeno, FontRef, GlyphId,
};
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::Window,
};
use zeno::{Format, Vector};

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct GlyphCacheKey {
    glyph_id: GlyphId,
    font_index: u32,
    size: u32,
    subpixel_offset_x: u8,
    subpixel_offset_y: u8,
}

impl GlyphCacheKey {
    fn new(glyph_id: GlyphId, font_index: u32, font_size: f32, subpixel_offset: Vector) -> Self {
        Self {
            glyph_id,
            font_index,
            size: (font_size * 10.0).trunc() as u32,
            subpixel_offset_x: (subpixel_offset.x * 10.0).trunc() as u8,
            subpixel_offset_y: (subpixel_offset.y * 10.0).trunc() as u8,
        }
    }
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
    rendered_glyphs: HashMap<GlyphCacheKey, Option<RenderedGlyph>>,
    glyph_textures: Vec<FontTexture>,
}

const TEXTURE_SIZE: usize = 512;

pub struct FontTexture {
    atlas: Atlas,
    image_id: ImageId,
}

fn run<W: WindowSurface>(mut canvas: Canvas<W::Renderer>, el: EventLoop<()>, mut surface: W, window: Arc<Window>) {
    // The text we are going to style and lay out
    let text = String::from(LOREM_TEXT);

    // The display scale for HiDPI rendering
    let display_scale = 1.0;

    // Colours for rendering
    let text_color = Color::rgb(0, 0, 0);

    // Create a FontContext, LayoutContext and ScaleContext
    //
    // These are all intended to be constructed rarely (perhaps even once per app (or once per thread))
    // and provide caches and scratch space to avoid allocations
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();
    let mut scale_cx = ScaleContext::new();

    // Setup some Parley text styles
    let brush_style = StyleProperty::Brush(text_color);
    let font_stack = FontStack::from("system-ui");

    // Creatse a RangedBuilder
    let mut builder = layout_cx.ranged_builder(&mut font_cx, &text, display_scale);

    // Set default text colour styles (set foreground text color)
    builder.push_default(brush_style);

    // Set default font family
    builder.push_default(font_stack);
    builder.push_default(StyleProperty::LineHeight(1.3));
    builder.push_default(StyleProperty::FontSize(16.0));

    // Build the builder into a Layout
    // let mut layout: Layout<Color> = builder.build(&text);
    let mut layout: Layout<Color> = builder.build(&text);

    let mut render_cache = RenderCache::default();

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
                WindowEvent::RedrawRequested { .. } => {
                    let size = window.inner_size();
                    canvas.set_size(size.width, size.height, 1.0);
                    canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.9, 0.9, 0.9));

                    let max_advance = Some(size.width as f32);

                    // Perform layout (including bidi resolution and shaping) with start alignment
                    layout.break_all_lines(max_advance);
                    layout.align(max_advance, Alignment::Start);

                    // Iterate over laid out lines
                    for line in layout.lines() {
                        // Iterate over GlyphRun's within each line
                        for item in line.items() {
                            match item {
                                PositionedLayoutItem::GlyphRun(glyph_run) => {
                                    render_glyph_run(&mut scale_cx, &mut render_cache, &glyph_run, &mut canvas);
                                }
                                PositionedLayoutItem::InlineBox(inline_box) => {
                                    let mut path = Path::new();
                                    path.rect(inline_box.x, inline_box.y, inline_box.width, inline_box.height);
                                    canvas.fill_path(&path, &Paint::color(Color::rgba(0, 0, 0, 255)));
                                }
                            };
                        }
                    }

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

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1000, 600, "Text demo", true);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

fn render_glyph_run<T: Renderer>(
    context: &mut ScaleContext,
    cache: &mut RenderCache,
    glyph_run: &GlyphRun<'_, Color>,
    canvas: &mut Canvas<T>,
) {
    let mut alpha_cmd_map = HashMap::new();
    let mut color_cmd_map = HashMap::new();

    // Resolve properties of the GlyphRun
    let mut run_x = glyph_run.offset();
    let run_y = glyph_run.baseline();
    let style = glyph_run.style();
    let color = style.brush;

    // Get the "Run" from the "GlyphRun"
    let run = glyph_run.run();

    // Resolve properties of the Run
    let font = run.font();
    let font_size = run.font_size();
    let normalized_coords = run.normalized_coords();

    // Convert from parley::Font to swash::FontRef
    let font_ref = FontRef::from_index(font.data.as_ref(), font.index as usize).unwrap();

    // Build a scaler. As the font properties are constant across an entire run of glyphs
    // we can build one scaler for the run and reuse it for each glyph.
    let mut scaler = context
        .builder(font_ref)
        .size(font_size)
        .hint(true)
        .normalized_coords(normalized_coords)
        .build();

    // Iterates over the glyphs in the GlyphRun
    for glyph in glyph_run.glyphs() {
        let glyph_x = run_x + glyph.x;
        let glyph_y = run_y - glyph.y;
        run_x += glyph.advance;

        // Compute the fractional offset
        // You'll likely want to quantize this in a real renderer
        let offset = Vector::new(glyph_x.fract(), glyph_y.fract());

        let cache_key = GlyphCacheKey::new(glyph.id, font.index, font_size, offset);

        let Some(rendered) = cache.rendered_glyphs.entry(cache_key).or_insert_with(|| {
            let (content, placement, is_color) = render_glyph(&mut scaler, glyph, offset);

            let content_w = placement.width as usize;
            let content_h = placement.height as usize;

            let mut found = None;
            for (texture_index, glyph_atlas) in cache.glyph_textures.iter_mut().enumerate() {
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
                let texture_index = cache.glyph_textures.len();
                let (x, y) = atlas.add_rect(content_w, content_h).unwrap();
                cache.glyph_textures.push(FontTexture { atlas, image_id });
                (texture_index, x, y)
            });

            canvas
                .update_image::<ImageSource>(
                    cache.glyph_textures[texture_index].image_id,
                    ImgRef::new(&content, content_w, content_h).into(),
                    atlas_alloc_x,
                    atlas_alloc_y,
                )
                .unwrap();

            Some(RenderedGlyph {
                texture_index,
                width: placement.width,
                height: placement.height,
                offset_x: placement.left,
                offset_y: placement.top,
                atlas_x: atlas_alloc_x as u32,
                atlas_y: atlas_alloc_y as u32,
                color_glyph: is_color,
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
            image_id: cache.glyph_textures[rendered.texture_index].image_id,
            quads: Vec::new(),
        });

        let mut q = Quad::default();
        let it = 1.0 / TEXTURE_SIZE as f32;

        q.x0 = glyph_x + rendered.offset_x as f32 - offset.x;
        q.y0 = glyph_y - rendered.offset_y as f32 - offset.y;
        q.x1 = q.x0 + rendered.width as f32;
        q.y1 = q.y0 + rendered.height as f32;

        q.s0 = rendered.atlas_x as f32 * it;
        q.t0 = rendered.atlas_y as f32 * it;
        q.s1 = (rendered.atlas_x + rendered.width) as f32 * it;
        q.t1 = (rendered.atlas_y + rendered.height) as f32 * it;

        cmd.quads.push(q);
    }

    canvas.draw_glyph_commands(
        GlyphDrawCommands {
            alpha_glyphs: alpha_cmd_map.into_values().collect(),
            color_glyphs: color_cmd_map.into_values().collect(),
        },
        &Paint::color(color),
    );
}

fn render_glyph(scaler: &mut Scaler<'_>, glyph: Glyph, offset: Vector) -> (Vec<RGBA8>, zeno::Placement, bool) {
    // Render the glyph using swash
    let rendered_glyph = Render::new(
        // Select our source order
        &[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ],
    )
    // Select the simple alpha (non-subpixel) format
    .format(Format::Alpha)
    // Apply the fractional offset
    .offset(offset)
    // Render the image
    .render(scaler, glyph.id)
    .unwrap();

    let glyph_width = rendered_glyph.placement.width as usize;
    let glyph_height = rendered_glyph.placement.height as usize;

    let mut src_buf = Vec::with_capacity(glyph_width * glyph_height);
    match rendered_glyph.content {
        Content::Mask => {
            for chunk in rendered_glyph.data.chunks_exact(1) {
                src_buf.push(RGBA8::new(chunk[0], 0, 0, 0));
            }
        }
        Content::Color => {
            for chunk in rendered_glyph.data.chunks_exact(4) {
                src_buf.push(RGBA8::new(chunk[0], chunk[1], chunk[2], chunk[3]));
            }
        }
        Content::SubpixelMask => unreachable!(),
    }

    (
        src_buf,
        rendered_glyph.placement,
        matches!(rendered_glyph.content, Content::Color),
    )
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
