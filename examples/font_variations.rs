use std::sync::Arc;

use femtovg::{Canvas, Color, Paint, Renderer};
use resource::resource;
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::KeyCode,
    window::Window,
};

mod helpers;
use helpers::WindowSurface;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(800, 700, "Font Variations", true);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

fn run<W: WindowSurface + 'static>(
    mut canvas: Canvas<W::Renderer>,
    mut surface: W,
    window: Arc<Window>,
) -> helpers::Callbacks {
    let font_id = canvas
        .add_font_mem(&resource!("examples/assets/RobotoFlex-VariableFont.ttf"))
        .expect("Cannot add font");

    let mut weight: f32 = 400.0;
    let mut slant: f32 = 0.0;

    helpers::Callbacks {
        window_event: Box::new(move |event, event_loop| match event {
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::Resized(physical_size) => {
                surface.resize(physical_size.width, physical_size.height);
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key: winit::keyboard::PhysicalKey::Code(keycode),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                match keycode {
                    KeyCode::ArrowUp => weight = (weight + 50.0).min(1000.0),
                    KeyCode::ArrowDown => weight = (weight - 50.0).max(100.0),
                    KeyCode::ArrowLeft => slant = (slant - 1.0).max(-10.0),
                    KeyCode::ArrowRight => slant = (slant + 1.0).min(0.0),
                    _ => {}
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let dpi_factor = window.scale_factor();
                let size = window.inner_size();
                canvas.set_size(size.width, size.height, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.95, 0.95, 0.95));

                draw_demo(&mut canvas, font_id, weight, slant);

                surface.present(&mut canvas);
            }
            _ => (),
        }),
        device_event: None,
    }
}

fn draw_demo<T: Renderer>(canvas: &mut Canvas<T>, font_id: femtovg::FontId, weight: f32, slant: f32) {
    let x = 40.0;
    let mut y = 40.0;

    // Title
    let title_paint = Paint::color(Color::rgbf(0.2, 0.2, 0.2))
        .with_font(&[font_id])
        .with_font_size(28.0)
        .with_font_weight(700.0);
    let _ = canvas.fill_text(x, y, "Font Variations Demo", &title_paint);
    y += 40.0;

    // Instructions
    let hint_paint = Paint::color(Color::rgbf(0.5, 0.5, 0.5))
        .with_font(&[font_id])
        .with_font_size(15.0);
    let _ = canvas.fill_text(x, y, "Up/Down: weight    Left/Right: slant", &hint_paint);
    y += 40.0;

    // Current values
    let label = format!("Weight: {}   Slant: {}", weight as i32, slant as i32);
    let dynamic_paint = Paint::color(Color::rgbf(0.0, 0.4, 0.8))
        .with_font(&[font_id])
        .with_font_size(32.0)
        .with_font_weight(weight)
        .with_font_slant(slant);
    let _ = canvas.fill_text(x, y, &label, &dynamic_paint);
    y += 50.0;

    // Sample text with both variations
    let sample_paint = Paint::color(Color::black())
        .with_font(&[font_id])
        .with_font_size(22.0)
        .with_font_weight(weight)
        .with_font_slant(slant);
    let _ = canvas.fill_text(x, y, "The quick brown fox jumps over the lazy dog", &sample_paint);
    y += 50.0;

    // --- Weight spectrum ---
    let section_paint = Paint::color(Color::rgbf(0.3, 0.3, 0.3))
        .with_font(&[font_id])
        .with_font_size(18.0)
        .with_font_weight(600.0);
    let _ = canvas.fill_text(x, y, "Weight axis (wght)", &section_paint);
    y += 30.0;

    let weights: &[(f32, &str)] = &[
        (100.0, "Thin 100"),
        (300.0, "Light 300"),
        (400.0, "Regular 400"),
        (500.0, "Medium 500"),
        (700.0, "Bold 700"),
        (900.0, "Black 900"),
    ];

    for &(w, label) in weights {
        let color = if (w - weight).abs() < 25.0 {
            Color::rgbf(0.0, 0.4, 0.8)
        } else {
            Color::black()
        };
        let paint = Paint::color(color)
            .with_font(&[font_id])
            .with_font_size(20.0)
            .with_font_weight(w)
            .with_font_slant(slant);
        let _ = canvas.fill_text(x, y, label, &paint);
        y += 30.0;
    }

    y += 10.0;

    // --- Slant spectrum ---
    let _ = canvas.fill_text(x, y, "Slant axis (slnt)", &section_paint);
    y += 30.0;

    let slants: &[(f32, &str)] = &[
        (0.0, "Upright (0)"),
        (-3.0, "Slight slant (-3)"),
        (-6.0, "Medium slant (-6)"),
        (-10.0, "Full slant (-10)"),
    ];

    for &(s, label) in slants {
        let color = if (s - slant).abs() < 0.5 {
            Color::rgbf(0.0, 0.4, 0.8)
        } else {
            Color::black()
        };
        let paint = Paint::color(color)
            .with_font(&[font_id])
            .with_font_size(20.0)
            .with_font_weight(weight)
            .with_font_slant(s);
        let _ = canvas.fill_text(x, y, label, &paint);
        y += 30.0;
    }
}
