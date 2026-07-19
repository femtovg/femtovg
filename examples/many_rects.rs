use std::sync::Arc;

/**
 * Draws a grid of rectangles and shows the per-frame cost, to compare what a
 * given number of drawn elements costs on each renderer.
 *
 * Build it against either backend:
 *
 *     cargo run --release --example many_rects
 *     cargo run --release --features wgpu --release --example many_rects
 *
 * The count comes from the first argument and defaults to 600. The difference
 * between the two backends is much larger on wasm, where every GPU resource
 * creation crosses the JS boundary into the browser's GPU process.
 */
use femtovg::{Canvas, Color, Paint, Path};
use helpers::WindowSurface;
use instant::Instant;
use winit::{event::WindowEvent, window::Window};

mod helpers;

const DEFAULT_COUNT: usize = 600;
const CELL: f32 = 30.0;
const SIZE: f32 = 26.0;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1200, 800, "many_rects example", false);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

fn rect_count() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::args()
            .nth(1)
            .and_then(|arg| arg.parse().ok())
            .unwrap_or(DEFAULT_COUNT)
    }
    #[cfg(target_arch = "wasm32")]
    {
        DEFAULT_COUNT
    }
}

fn run<W: WindowSurface + 'static>(
    mut canvas: Canvas<W::Renderer>,
    mut surface: W,
    window: Arc<Window>,
) -> helpers::Callbacks {
    let count = rect_count();
    let mut perf = helpers::PerfGraph::new();
    let mut previous_frame = Instant::now();

    helpers::Callbacks {
        window_event: Box::new(move |event, _| match event {
            WindowEvent::Resized(physical_size) => {
                surface.resize(physical_size.width, physical_size.height);
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                perf.update((now - previous_frame).as_secs_f32());
                previous_frame = now;

                let size = window.inner_size();
                let dpi = window.scale_factor() as f32;
                canvas.set_size(size.width, size.height, dpi);
                canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.06, 0.07, 0.09));

                let per_row = ((size.width as f32 / dpi - 16.0) / CELL).max(1.0) as usize;
                let fill = Paint::color(Color::rgbf(0.18, 0.27, 0.77));
                for i in 0..count {
                    let x = 8.0 + (i % per_row) as f32 * CELL;
                    let y = 8.0 + (i / per_row) as f32 * CELL;
                    let mut path = Path::new();
                    path.rounded_rect(x, y, SIZE, SIZE, 6.0);
                    canvas.fill_path(&path, &fill);
                }

                canvas.save();
                canvas.reset();
                perf.render(&mut canvas, 5.0, 5.0);
                canvas.restore();

                surface.present(&mut canvas);
                // Keep drawing so the graph reflects a steady state rather than
                // whatever the last resize happened to cost.
                window.request_redraw();
            }
            _ => (),
        }),
        device_event: None,
    }
}
