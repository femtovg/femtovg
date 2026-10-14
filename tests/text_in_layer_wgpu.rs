//! Headless GPU regression test: stroked text drawn inside a layer renders the
//! same whether or not its glyphs are already in the atlas.
//!
//! Glyphs that are not cached are rasterized into the atlas texture during the
//! draw. That must not close the layer or change the render target. Stroked
//! text is used because it takes the path-glyph code in every feature set.
//! The test returns early when no GPU adapter is available.
#![cfg(all(feature = "wgpu", feature = "textlayout"))]

use femtovg::{renderer::WGPURenderer, Canvas, Color, LayerEffects, Paint};

mod common;
use common::{headless_device, render_rgba};

const W: u32 = 256;
const H: u32 = 96;
const FONT: &[u8] = include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf");

fn stroke_text(canvas: &mut Canvas<WGPURenderer>, paint: &Paint) {
    canvas.stroke_text(4.0, 60.0, "Layer", paint).expect("stroke_text");
}

#[test]
fn stroked_text_in_a_layer_draws_alike_on_a_cold_and_a_warm_atlas() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let frame = |warm: bool| {
        render_rgba(&device, &queue, W, H, Color::white(), |canvas| {
            let font = canvas.add_font_mem(FONT).expect("font");
            let paint = Paint::color(Color::black())
                .with_font(&[font])
                .with_font_size(48.0)
                .with_line_width(2.0);
            if warm {
                // Draw once to fill the atlas, then clear the canvas.
                stroke_text(canvas, &paint);
                canvas.clear_rect(0, 0, W, H, Color::white());
            }
            canvas.save();
            canvas.translate(8.0, 4.0);
            assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.75)));
            stroke_text(canvas, &paint);
            canvas.end_layer();
            canvas.restore();
        })
    };

    let (cold, warm) = (frame(false), frame(true));

    let ink = warm.chunks_exact(4).filter(|px| px[0] < 160).count();
    assert!(ink > 200, "the warm frame shows the text: {ink} inked pixels");
    let differing = cold
        .chunks_exact(4)
        .zip(warm.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "pixels differ between the cold-atlas and the warm-atlas frame"
    );
}
