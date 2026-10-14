//! Tests that drawing text through the glyph atlas does not change the caller's
//! canvas: open layers, state stack, current state and render target.

use super::*;
use crate::{
    budgets::MAX_STATE_DEPTH,
    renderer::{CommandType, Vertex},
    LayerEffects, RecordingRenderer,
};

const FONT: &[u8] = include_bytes!("../../examples/assets/RobotoFlex-VariableFont.ttf");

type TestCanvas = Canvas<RecordingRenderer>;

fn canvas() -> (TestCanvas, FontId) {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(400, 200, 1.0);
    let font = canvas.text_context.borrow_mut().add_font_mem(FONT).unwrap();
    (canvas, font)
}

/// Returns a glyph run for `text`: one glyph per character, 30px apart.
fn glyphs(text: &str) -> Vec<PositionedGlyph> {
    let charmap = swash::FontRef::from_index(FONT, 0).unwrap().charmap();
    text.chars()
        .enumerate()
        .map(|(index, c)| PositionedGlyph {
            x: 10.0 + 30.0 * index as f32,
            y: 70.0,
            glyph_id: charmap.map(c),
        })
        .collect()
}

/// The render target the test draws text on.
#[derive(Clone, Copy, Debug)]
enum Target {
    Screen,
    Image,
    Layer,
}

const TARGETS: [Target; 3] = [Target::Screen, Target::Image, Target::Layer];

/// Sets non-default state (a saved level, transform, alpha, scissor and blend
/// mode) and switches to `target`.
fn enter(canvas: &mut TestCanvas, target: Target) {
    canvas.save();
    canvas.translate(7.0, 11.0);
    canvas.set_global_alpha(0.5);
    canvas.scissor(0.0, 0.0, 300.0, 150.0);
    canvas.global_composite_operation(crate::CompositeOperation::Lighter);
    match target {
        Target::Screen => {}
        Target::Image => {
            let image = canvas
                .create_image_empty(400, 200, PixelFormat::Rgba8, ImageFlags::empty())
                .unwrap();
            canvas.set_render_target(RenderTarget::Image(image));
        }
        Target::Layer => {
            assert!(canvas.begin_layer(&LayerEffects::new()));
            assert!(matches!(canvas.current_render_target, RenderTarget::Image(_)));
        }
    }
}

/// Snapshot of the canvas state that a text draw must not change.
#[derive(Debug, PartialEq)]
struct CallerState {
    /// Debug output of the open layers.
    layers: String,
    /// Lengths of the state stack and of the overflow list.
    depth: (usize, usize),
    /// Debug output of the current state.
    state: String,
    /// `Canvas::current_render_target`.
    target: RenderTarget,
    /// The last render target set in the command queue.
    queued_target: RenderTarget,
}

impl CallerState {
    fn of(canvas: &TestCanvas) -> Self {
        Self {
            layers: format!("{:?}", canvas.layers),
            depth: (canvas.state_stack.len(), canvas.overflow.len()),
            state: format!("{:?}", canvas.state()),
            target: canvas.current_render_target,
            queued_target: canvas
                .commands
                .iter()
                .rev()
                .find_map(|command| match command.cmd_type {
                    CommandType::SetRenderTarget(target) => Some(target),
                    _ => None,
                })
                .unwrap_or(RenderTarget::Screen),
        }
    }
}

/// Runs `draw` and asserts that `CallerState` is the same before and after.
fn keeping_caller_state<R>(canvas: &mut TestCanvas, context: &str, draw: impl FnOnce(&mut TestCanvas) -> R) -> R {
    let before = CallerState::of(canvas);
    let result = draw(canvas);
    assert_eq!(CallerState::of(canvas), before, "{context}");
    result
}

/// One glyph mask in the command queue: the atlas image, the cleared rect as
/// `[x0, y0, x1, y1]`, and the vertices of each sample drawn after the clear.
#[derive(Debug, PartialEq)]
struct QueuedMask {
    image: ImageId,
    clear: [f32; 4],
    samples: Vec<Vec<Vertex>>,
}

/// Parses the command queue into the masks drawn on atlas textures, in order.
fn queued_masks(canvas: &TestCanvas) -> Vec<QueuedMask> {
    let textures = canvas.glyph_atlas.glyph_textures.borrow();
    let verts =
        |range: Option<(usize, usize)>| range.map_or(&[][..], |(start, count)| &canvas.verts[start..start + count]);
    let mut atlas = None;
    let mut masks = Vec::new();
    for command in &canvas.commands {
        match command.cmd_type {
            CommandType::SetRenderTarget(target) => {
                atlas = textures
                    .iter()
                    .map(|texture| texture.image_id)
                    .find(|image| target == RenderTarget::Image(*image));
            }
            CommandType::ClearRect { .. } => {
                let Some(image) = atlas else { continue };
                let corners = verts(command.triangles_verts);
                let bound = |pick: fn(&Vertex) -> f32, fold: fn(f32, f32) -> f32| {
                    corners.iter().map(pick).reduce(fold).unwrap()
                };
                masks.push(QueuedMask {
                    image,
                    clear: [
                        bound(|v| v.x, f32::min),
                        bound(|v| v.y, f32::min),
                        bound(|v| v.x, f32::max),
                        bound(|v| v.y, f32::max),
                    ],
                    samples: Vec::new(),
                });
            }
            _ => {
                if let (Some(_), Some(mask)) = (atlas, masks.last_mut()) {
                    let mut sample = verts(command.triangles_verts).to_vec();
                    for drawable in &command.drawables {
                        sample.extend_from_slice(verts(drawable.fill_verts));
                        sample.extend_from_slice(verts(drawable.stroke_verts));
                    }
                    mask.samples.push(sample);
                }
            }
        }
    }
    masks
}

/// Returns a cached glyph's atlas cell in `clear_rect()` coordinates, where y is
/// measured from the bottom of the texture.
fn cell(glyph: &RenderedGlyph) -> [f32; 4] {
    let x0 = glyph.atlas_x as f32;
    let y0 = (TEXTURE_SIZE as u32 - glyph.atlas_y - glyph.height) as f32;
    [x0, y0, x0 + glyph.width as f32, y0 + glyph.height as f32]
}

/// Asserts that every cached glyph has a clear and eight samples queued for its
/// cell. A cached glyph without them would be drawn from an empty cell. Only
/// valid when every cached glyph is a path glyph, as with stroked text.
fn assert_every_cached_glyph_has_its_mask_queued(canvas: &TestCanvas) {
    let masks = queued_masks(canvas);
    let textures = canvas.glyph_atlas.glyph_textures.borrow();
    let margin = GLYPH_MARGIN as f32;
    for glyph in canvas.glyph_atlas.rendered_glyphs.borrow().values().flatten() {
        let [x0, y0, x1, y1] = cell(glyph);
        let cleared = [x0 - margin, y0 - margin, x1 + margin, y1 + margin];
        let image = textures[glyph.texture_index].image_id;
        let mask = masks.iter().find(|mask| mask.image == image && mask.clear == cleared);
        assert!(
            mask.is_some_and(|mask| mask.samples.len() == 8 && mask.samples.iter().all(|sample| !sample.is_empty())),
            "{glyph:?} is cached but has no clear and eight samples queued for {cleared:?}"
        );
    }
}

fn stroke_paint() -> Paint {
    Paint::color(Color::black()).with_font_size(24.0).with_line_width(1.5)
}

/// A stroke wide enough that "." still fits in an atlas texture but "W" does not.
fn oversized_stroke_paint() -> Paint {
    Paint::color(Color::black()).with_font_size(40.0).with_line_width(480.0)
}

/// Swash uploads glyphs immediately, but a queued `ClearRect` runs at flush.
/// A clear that overlaps an uploaded glyph would erase it.
#[cfg(all(feature = "swash", feature = "debug_inspector"))]
#[test]
fn no_queued_clear_covers_an_uploaded_glyph() {
    let (mut canvas, font) = canvas();
    let paint = Paint::color(Color::black()).with_font_size(24.0);
    canvas.fill_glyph_run(font, &[], glyphs("Atlas"), &paint).unwrap();

    let rendered = canvas.glyph_atlas.rendered_glyphs.borrow();
    let textures = canvas.glyph_atlas.glyph_textures.borrow();
    assert_eq!(rendered.values().flatten().count(), 5, "every glyph is in the atlas");
    for glyph in rendered.values().flatten() {
        let [left, top, right, bottom] = cell(glyph);
        let image = textures[glyph.texture_index].image_id;
        for mask in queued_masks(&canvas).iter().filter(|mask| mask.image == image) {
            let [x0, y0, x1, y1] = mask.clear;
            assert!(
                x1 <= left || right <= x0 || y1 <= top || bottom <= y0,
                "ClearRect ({x0}, {y0})..({x1}, {y1}) overlaps the uploaded glyph at ({left}, {top})..({right}, {bottom})"
            );
        }
    }
}

/// Stroked text is used because it takes the path-glyph code in every feature set.
#[test]
fn stroking_text_keeps_the_callers_state() {
    for target in TARGETS {
        let (mut canvas, font) = canvas();
        enter(&mut canvas, target);
        for atlas in ["cold", "warm"] {
            keeping_caller_state(&mut canvas, &format!("{target:?}, {atlas} atlas"), |canvas| {
                canvas
                    .stroke_glyph_run(font, &[], glyphs("Atlas"), &stroke_paint())
                    .unwrap();
            });
        }
        assert_eq!(queued_masks(&canvas).len(), 5, "the cold draw queued every mask");
        assert_every_cached_glyph_has_its_mask_queued(&canvas);
    }
}

/// A run where "." fits in the atlas and "W" then fails as too large. The draw
/// returns the error, the caller's canvas is unchanged, and the mask for "."
/// is still queued, so the next draw of "." uses the cached glyph.
#[test]
fn a_failed_draw_keeps_the_callers_state_and_the_masks_placed_before_it() {
    let paint = oversized_stroke_paint();

    for target in TARGETS {
        for warm in [false, true] {
            let context = format!("{target:?}, {} atlas", if warm { "warm" } else { "cold" });
            let (mut canvas, font) = canvas();
            if warm {
                canvas
                    .stroke_glyph_run(font, &[], glyphs("Atlas"), &stroke_paint())
                    .unwrap();
            }
            let cached = canvas.glyph_atlas.rendered_glyphs.borrow().len();
            enter(&mut canvas, target);

            let result = keeping_caller_state(&mut canvas, &context, |canvas| {
                canvas.stroke_glyph_run(font, &[], glyphs(".W"), &paint)
            });

            assert!(
                matches!(result, Err(ErrorKind::FontSizeTooLargeForAtlas)),
                "{context}: {result:?}"
            );
            assert_eq!(
                canvas.glyph_atlas.rendered_glyphs.borrow().len(),
                cached + 1,
                "{context}: \".\" was placed before \"W\" failed"
            );
            assert_every_cached_glyph_has_its_mask_queued(&canvas);

            let queued = canvas.commands.len();
            keeping_caller_state(&mut canvas, &context, |canvas| {
                canvas.stroke_glyph_run(font, &[], glyphs("."), &paint).unwrap();
            });
            assert_eq!(
                canvas.commands.len() - queued,
                1,
                "{context}: the next draw should queue only the glyph quad"
            );
        }
    }
}

/// Returns a canvas with `depth` saved levels, `past` of them beyond `MAX_STATE_DEPTH`.
fn canvas_at_depth(depth: usize, past: usize) -> (TestCanvas, FontId) {
    let (mut canvas, font) = canvas();
    while canvas.state_stack.len() + canvas.overflow.len() < depth {
        canvas.save();
    }
    assert_eq!((canvas.state_stack.len(), canvas.overflow.len()), (depth - past, past));
    (canvas, font)
}

/// A mask is cached, so it has to be drawn correctly even when the state stack
/// is at or past `MAX_STATE_DEPTH`, where the caller's own draws are dropped.
/// The caller's canvas is unchanged there too.
#[test]
fn stroking_text_at_the_depth_limit_draws_the_mask_and_keeps_the_callers_state() {
    let masks_at = |depth: usize, past: usize| {
        let (mut canvas, font) = canvas_at_depth(depth, past);
        keeping_caller_state(&mut canvas, &format!("{past} past the limit"), |canvas| {
            // The second draw finds the glyph cached.
            for _ in 0..2 {
                canvas
                    .stroke_glyph_run(font, &[], glyphs("g"), &stroke_paint())
                    .unwrap();
            }
        });
        queued_masks(&canvas)
    };
    let at_root = masks_at(1, 0);
    assert_eq!(at_root.len(), 1);
    assert_eq!(at_root[0].samples.len(), 8);
    assert!(at_root[0].samples.iter().all(|sample| !sample.is_empty()));

    for past in [0, 1, 3] {
        assert!(
            masks_at(MAX_STATE_DEPTH + past, past) == at_root,
            "{past} past the limit: mask differs from the one drawn at depth 1"
        );
    }
}

/// All masks for a run are drawn in one switch to the atlas texture and back,
/// before the quad that samples them.
#[test]
fn a_cold_run_visits_the_atlas_once() {
    let (mut canvas, font) = canvas();
    let queued = canvas.commands.len();
    canvas
        .stroke_glyph_run(font, &[], glyphs("Batch"), &stroke_paint())
        .unwrap();

    let atlas = RenderTarget::Image(canvas.glyph_atlas.glyph_textures.borrow()[0].image_id);
    let kinds: String = canvas.commands[queued..]
        .iter()
        .map(|command| match command.cmd_type {
            CommandType::SetRenderTarget(target) if target == atlas => 'A',
            CommandType::SetRenderTarget(RenderTarget::Screen) => 'S',
            CommandType::SetRenderTarget(_) => '?',
            CommandType::ClearRect { .. } => 'c',
            CommandType::Triangles { .. } => 'q',
            _ => 'm',
        })
        .collect();
    let mask = "cmmmmmmmm";
    assert_eq!(kinds, format!("A{}Sq", mask.repeat(5)));
}

/// When a run's masks land in two atlas textures, each mask is drawn on the
/// texture its glyph was placed in.
#[test]
fn a_run_spilling_into_a_second_atlas_texture_draws_each_mask_on_its_own() {
    let (mut canvas, font) = canvas();
    canvas
        .stroke_glyph_run(font, &[], glyphs(".,"), &oversized_stroke_paint())
        .unwrap();

    assert_eq!(canvas.glyph_atlas.glyph_textures.borrow().len(), 2);
    let masks = queued_masks(&canvas);
    assert_eq!(masks.len(), 2);
    assert_ne!(masks[0].image, masks[1].image);
    assert_every_cached_glyph_has_its_mask_queued(&canvas);
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
}

/// If the debug fill upload fails, the draw returns the error, the texture is
/// not registered and its image is deleted.
#[cfg(feature = "debug_inspector")]
#[test]
fn a_failed_debug_fill_releases_the_new_atlas_texture() {
    let (mut canvas, font) = canvas();
    let paint = Paint::color(Color::black()).with_font_size(24.0);
    let allocated = canvas.renderer.image_allocation_attempts;
    canvas.renderer.fail_image_updates = true;

    let result = keeping_caller_state(&mut canvas, "failed fill", |canvas| {
        canvas.fill_glyph_run(font, &[], glyphs("A"), &paint)
    });

    assert!(matches!(result, Err(ErrorKind::UnknownError)), "{result:?}");
    assert!(canvas.glyph_atlas.glyph_textures.borrow().is_empty());
    assert!(canvas.glyph_atlas.rendered_glyphs.borrow().is_empty());
    canvas.flush_to_output(());
    assert_eq!(canvas.renderer.image_allocation_attempts - allocated, 1);
    assert_eq!(
        canvas.renderer.image_deletion_count, 1,
        "the texture's image is released"
    );
}

/// If the upload of a PNG glyph fails, the draw returns the error and the
/// caller's canvas is unchanged.
#[cfg(all(feature = "textlayout", feature = "image-loading"))]
#[test]
fn a_failed_png_glyph_upload_fails_the_draw_and_keeps_the_callers_state() {
    let paint = stroke_paint();
    let canvas_with_png_font = || {
        let (canvas, font) = canvas();
        let png_font = canvas
            .text_context
            .borrow_mut()
            .add_font_mem(&test_fonts::png_glyph_font())
            .unwrap();
        (canvas, font, png_font)
    };
    // Glyph 0 is the only glyph in `png_glyph_font()`.
    let png_glyph = || {
        [PositionedGlyph {
            x: 40.0,
            y: 70.0,
            glyph_id: 0,
        }]
    };

    let (mut canvas, _, png_font) = canvas_with_png_font();
    canvas.stroke_glyph_run(png_font, &[], png_glyph(), &paint).unwrap();
    let cached: Vec<bool> = canvas
        .glyph_atlas
        .rendered_glyphs
        .borrow()
        .values()
        .flatten()
        .map(|glyph| glyph.color_glyph)
        .collect();
    assert_eq!(cached, [true], "the PNG glyph is uploaded as an image");

    for target in TARGETS {
        let (mut canvas, font, png_font) = canvas_with_png_font();
        // Draw once so the atlas texture exists before uploads start failing.
        // Otherwise, with debug_inspector, the texture's debug fill would fail first.
        canvas.stroke_glyph_run(font, &[], glyphs("g"), &paint).unwrap();
        enter(&mut canvas, target);
        canvas.renderer.fail_image_updates = true;

        let result = keeping_caller_state(&mut canvas, &format!("{target:?}"), |canvas| {
            canvas.stroke_glyph_run(png_font, &[], png_glyph(), &paint)
        });

        assert!(matches!(result, Err(ErrorKind::UnknownError)), "{target:?}: {result:?}");
        assert_eq!(
            canvas.glyph_atlas.rendered_glyphs.borrow().len(),
            1,
            "only \"g\" is cached"
        );
        assert_every_cached_glyph_has_its_mask_queued(&canvas);
    }
}
