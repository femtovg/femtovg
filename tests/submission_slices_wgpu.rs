#![cfg(feature = "wgpu")]
//! A frame that opens hundreds of layers is submitted in slices of render
//! passes: Metal keeps driver memory for every pass until its command buffer
//! completes, and one command buffer per frame reached gigabytes (1,600
//! opacity layers failed buffer creation on an M4 Max; a 176-layer portrait
//! was jetsammed on an iPhone 12). Every layer's uniforms and draws have to
//! land in the slice that runs them, in order.

use femtovg::{renderer::WGPURenderer, Canvas, Color, LayerEffects, Paint, Path};

mod common;
use common::{headless_device, render_rgba};

type C = Canvas<WGPURenderer>;

fn fill(c: &mut C, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let mut p = Path::new();
    p.rect(x, y, w, h);
    let mut paint = Paint::color(color);
    paint.set_anti_alias(false);
    c.fill_path(&p, &paint);
}

fn cell(i: usize) -> [u8; 3] {
    [(i * 37 % 256) as u8, (i * 91 % 256) as u8, (i * 53 % 256) as u8]
}

fn half_over_white(c: u8) -> i32 {
    (i32::from(c) + 255 + 1) / 2
}

#[test]
fn every_layer_of_a_long_frame_lands_in_its_slice() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    const W: u32 = 64;
    const H: u32 = 5;
    // One layer per pixel: 320 layers open at least 640 passes, ten slices.
    let layers = (W * H) as usize;
    let out = render_rgba(&device, &queue, W, H, Color::white(), |c| {
        let half = LayerEffects::new().with_opacity(0.5);
        for i in 0..layers {
            assert!(c.begin_layer(&half));
            let [r, g, b] = cell(i);
            fill(c, (i % W as usize) as f32, (i / W as usize) as f32, 1.0, 1.0, Color::rgb(r, g, b));
            c.end_layer();
        }
    });
    for i in 0..layers {
        let got = &out[i * 4..i * 4 + 3];
        let want = cell(i).map(half_over_white);
        for ch in 0..3 {
            assert!(
                (i32::from(got[ch]) - want[ch]).abs() <= 1,
                "layer {i}: pixel {got:?}, expected {want:?} at half opacity over white"
            );
        }
    }
}

#[test]
fn two_thousand_layers_in_one_frame_render() {
    let Some((device, queue)) = headless_device() else {
        return;
    };
    let out = render_rgba(&device, &queue, 16, 16, Color::white(), |c| {
        let half = LayerEffects::new().with_opacity(0.5);
        for _ in 0..2000 {
            assert!(c.begin_layer(&half));
            fill(c, 0.0, 0.0, 16.0, 16.0, Color::rgb(0, 0, 255));
            c.end_layer();
        }
    });
    // Each half-blue layer halves the distance to blue; rounding holds the
    // red and green channels at one.
    assert!(out[0] <= 1 && out[1] <= 1 && out[2] == 255, "pixel {:?}", &out[..3]);
}
