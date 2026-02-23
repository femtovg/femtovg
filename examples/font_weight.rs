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
    helpers::start(800, 600, "Variable Font Weight", true);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

fn run<W: WindowSurface + 'static>(
    mut canvas: Canvas<W::Renderer>,
    mut surface: W,
    window: Arc<Window>,
) -> helpers::Callbacks {
    let font_id = canvas
        .add_font_mem(&resource!("examples/assets/Roboto-VariableFont_wght.ttf"))
        .expect("Cannot add font");

    let mut weight: f32 = 400.0;

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
                    KeyCode::ArrowUp => weight = (weight + 50.0).min(900.0),
                    KeyCode::ArrowDown => weight = (weight - 50.0).max(100.0),
                    _ => {}
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let dpi_factor = window.scale_factor();
                let size = window.inner_size();
                canvas.set_size(size.width, size.height, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.95, 0.95, 0.95));

                draw_weight_demo(&mut canvas, font_id, weight);

                surface.present(&mut canvas);
            }
            _ => (),
        }),
        device_event: None,
    }
}

fn draw_weight_demo<T: Renderer>(canvas: &mut Canvas<T>, font_id: femtovg::FontId, current_weight: f32) {
    let x = 40.0;
    let mut y = 40.0;

    // Title
    let title_paint = Paint::color(Color::rgbf(0.2, 0.2, 0.2))
        .with_font(&[font_id])
        .with_font_size(28.0)
        .with_font_weight(700.0);
    let _ = canvas.fill_text(x, y, "Variable Font Weight Demo", &title_paint);
    y += 50.0;

    // Instructions
    let hint_paint = Paint::color(Color::rgbf(0.5, 0.5, 0.5))
        .with_font(&[font_id])
        .with_font_size(16.0);
    let _ = canvas.fill_text(x, y, "Press Up/Down arrow keys to change weight", &hint_paint);
    y += 50.0;

    // Current dynamic weight
    let label = format!("Current weight: {}", current_weight as u32);
    let dynamic_paint = Paint::color(Color::rgbf(0.0, 0.4, 0.8))
        .with_font(&[font_id])
        .with_font_size(36.0)
        .with_font_weight(current_weight);
    let _ = canvas.fill_text(x, y, &label, &dynamic_paint);
    y += 60.0;

    // Sample text at current weight
    let sample_paint = Paint::color(Color::black())
        .with_font(&[font_id])
        .with_font_size(24.0)
        .with_font_weight(current_weight);
    let _ = canvas.fill_text(x, y, "The quick brown fox jumps over the lazy dog", &sample_paint);
    y += 60.0;

    // Fixed weight spectrum
    let weights: &[(f32, &str)] = &[
        (100.0, "Thin 100"),
        (200.0, "ExtraLight 200"),
        (300.0, "Light 300"),
        (400.0, "Regular 400"),
        (500.0, "Medium 500"),
        (600.0, "SemiBold 600"),
        (700.0, "Bold 700"),
        (800.0, "ExtraBold 800"),
        (900.0, "Black 900"),
    ];

    for &(w, label) in weights {
        let color = if (w - current_weight).abs() < 1.0 {
            Color::rgbf(0.0, 0.4, 0.8)
        } else {
            Color::black()
        };
        let paint = Paint::color(color)
            .with_font(&[font_id])
            .with_font_size(22.0)
            .with_font_weight(w);
        let _ = canvas.fill_text(x, y, label, &paint);
        y += 34.0;
    }
}
