use std::time::Instant;

use femtovg::{renderer::OpenGl, Align, Baseline, Canvas, Color, Paint, Path, Renderer};
use glutin::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    ContextBuilder,
};

fn main() {
    let window_size = glutin::dpi::PhysicalSize::new(1000, 670);
    let el = EventLoop::new();
    let wb = WindowBuilder::new()
        .with_inner_size(window_size)
        .with_resizable(false)
        .with_title("Gradient test");

    let windowed_context = ContextBuilder::new().build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new_from_glutin_context(&windowed_context).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    canvas.set_size(
        window_size.width as u32,
        window_size.height as u32,
        windowed_context.window().scale_factor() as f32,
    );
    canvas
        .add_font("examples/assets/Roboto-Regular.ttf")
        .expect("Cannot add font");

    let start = Instant::now();
    let mut prevt = start;

    let mut perf = PerfGraph::new();

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => (),
            },
            Event::RedrawRequested(_) => {
                let dpi_factor = windowed_context.window().scale_factor();
                let size = windowed_context.window().inner_size();
                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.9, 0.9, 0.9));

                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                perf.update(dt);

                draw_gradients(&mut canvas);

                canvas.save();
                canvas.reset();
                perf.render(&mut canvas, 5.0, 5.0);
                canvas.restore();

                canvas.flush();
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => windowed_context.window().request_redraw(),
            _ => (),
        }
    });
}

fn draw_gradients<T: Renderer>(canvas: &mut Canvas<T>) {
    let mut r = |x, y, name, paint| {
        let mut p = Path::new();
        p.rect(0.0, 0.0, 100.0, 100.0);
        canvas.translate(x, y);
        canvas.fill_path(&mut p, &paint);
        canvas.translate(-x, -y);
        let mut text_paint = Paint::color(Color::black());
        text_paint.set_font_size(14.0);
        let _ = canvas.fill_text(x, y + 114.0, name, &text_paint);
    };
    // Various two stop gradients
    let mut x = 10.0;
    let mut y = 10.0;

    // OPAQUE LINEAR
    r(
        x,
        y,
        "x linear opaque",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            0.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "y linear opaque",
        Paint::linear_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "xy linear opaque",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            100.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );

    // 50% LINEAR
    x += 110.0;
    r(
        x,
        y,
        "x linear 50%",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            0.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "y linear 50%",
        Paint::linear_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "xy linear 50%",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            100.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );

    // TRANSPARENT TO OPAQUE LINEAR
    x += 110.0;
    r(
        x,
        y,
        "x linear 0-100",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            0.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "y linear 0-100",
        Paint::linear_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "xy linear 0-100",
        Paint::linear_gradient(
            0.0,
            0.0,
            100.0,
            100.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 255),
        ),
    );

    y += 130.0;
    x = 10.0;
    // OPAQUE RADIAL
    r(
        x,
        y,
        "radial opaque",
        Paint::radial_gradient(
            50.0,
            50.0,
            0.0,
            50.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "0,0 rad opaque",
        Paint::radial_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "fill rad opaque",
        Paint::radial_gradient(
            50.0,
            50.0,
            25.0,
            75.0,
            Color::rgba(255, 0, 0, 255),
            Color::rgba(0, 0, 255, 255),
        ),
    );

    // 50% LINEAR
    x += 110.0;
    r(
        x,
        y,
        "radial 50%",
        Paint::radial_gradient(
            50.0,
            50.0,
            0.0,
            50.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "0,0 rad 50%",
        Paint::radial_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "fill rad 50%",
        Paint::radial_gradient(
            50.0,
            50.0,
            25.0,
            75.0,
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 0, 255, 128),
        ),
    );

    // TRANSPARENT TO OPAQUE LINEAR
    x += 110.0;
    r(
        x,
        y,
        "radial 0-100",
        Paint::radial_gradient(
            50.0,
            50.0,
            0.0,
            50.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "0,0 rad 0-100",
        Paint::radial_gradient(
            0.0,
            0.0,
            0.0,
            100.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 128),
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "fill rad 0-100",
        Paint::radial_gradient(
            50.0,
            50.0,
            25.0,
            75.0,
            Color::rgba(255, 0, 0, 0),
            Color::rgba(0, 0, 255, 128),
        ),
    );

    // Multistop!
    y += 130.0;
    x = 10.0;
    r(
        x,
        y,
        "ms x linear op",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms y linear op",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms xy linear op",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    // Multistop linear 50%
    x += 110.0;
    r(
        x,
        y,
        "ms x linear 50%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms y linear 50%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms xy linear 50%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    // Multistop linear transparent & opaque
    x += 110.0;
    r(
        x,
        y,
        "ms x linear 0-100",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms y linear 0-100",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms xy linear 0-100%",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    // Multistop radial
    y += 130.0;
    x = 10.0;
    r(
        x,
        y,
        "ms radial opq",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            0.0,
            50.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms 0,0 rad opq",
        Paint::radial_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms rad opq",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            25.0,
            75.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 255)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
    // Multistop radial 50%
    x += 110.0;
    r(
        x,
        y,
        "ms radial 50%",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            0.0,
            50.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms 0,0 rad 50%",
        Paint::radial_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms rad 50%",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            25.0,
            75.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 128)),
                (0.5, Color::rgba(0, 255, 0, 128)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    // Multistop radial transparent
    x += 110.0;
    r(
        x,
        y,
        "ms radial 0-100",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            0.0,
            50.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms 0,0 rad 0-100",
        Paint::radial_gradient_stops(
            0.0,
            0.0,
            0.0,
            100.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms rad 0-100",
        Paint::radial_gradient_stops(
            50.0,
            50.0,
            25.0,
            75.0,
            &[
                (0.0, Color::rgba(255, 0, 0, 0)),
                (0.5, Color::rgba(0, 255, 0, 255)),
                (1.0, Color::rgba(0, 0, 255, 128)),
            ],
        ),
    );

    // Multistop padding cases
    x = 10.0;
    y += 130.0;
    r(
        x,
        y,
        "ms pad start",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[(0.5, Color::rgba(255, 0, 0, 255)), (1.0, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms pad both",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[(0.4, Color::rgba(255, 0, 0, 255)), (0.6, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms pad end",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[(0.0, Color::rgba(255, 0, 0, 255)), (0.5, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms same stop",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[(0.5, Color::rgba(255, 0, 0, 255)), (0.5, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms one stop",
        Paint::linear_gradient_stops(0.0, 0.0, 100.0, 0.0, &[(0.5, Color::rgba(255, 0, 0, 255))]),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms zero stops",
        Paint::linear_gradient_stops(0.0, 0.0, 100.0, 0.0, &[]),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms non-seq 1",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[(0.5, Color::rgba(255, 0, 0, 255)), (0.0, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms non-seq 2",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[(0.5, Color::rgba(255, 0, 0, 255)), (0.3, Color::rgba(0, 0, 255, 255))],
        ),
    );
    x += 110.0;
    r(
        x,
        y,
        "ms non-seq 3",
        Paint::linear_gradient_stops(
            0.0,
            0.0,
            100.0,
            0.0,
            &[
                (0.5, Color::rgba(255, 0, 0, 255)),
                (0.6, Color::rgba(0, 255, 0, 255)),
                (0.3, Color::rgba(0, 0, 255, 255)),
            ],
        ),
    );
}

struct PerfGraph {
    history_count: usize,
    values: Vec<f32>,
    head: usize,
}

impl PerfGraph {
    fn new() -> Self {
        Self {
            history_count: 100,
            values: vec![0.0; 100],
            head: Default::default(),
        }
    }

    fn update(&mut self, frame_time: f32) {
        self.head = (self.head + 1) % self.history_count;
        self.values[self.head] = frame_time;
    }

    fn get_average(&self) -> f32 {
        self.values.iter().map(|v| *v).sum::<f32>() / self.history_count as f32
    }

    fn render<T: Renderer>(&self, canvas: &mut Canvas<T>, x: f32, y: f32) {
        let avg = self.get_average();

        let w = 200.0;
        let h = 35.0;

        let mut path = Path::new();
        path.rect(x, y, w, h);
        canvas.fill_path(&mut path, &Paint::color(Color::rgba(0, 0, 0, 128)));

        let mut path = Path::new();
        path.move_to(x, y + h);

        for i in 0..self.history_count {
            let mut v = 1.0 / (0.00001 + self.values[(self.head + i) % self.history_count]);
            if v > 80.0 {
                v = 80.0;
            }
            let vx = x + (i as f32 / (self.history_count - 1) as f32) * w;
            let vy = y + h - ((v / 80.0) * h);
            path.line_to(vx, vy);
        }

        path.line_to(x + w, y + h);
        canvas.fill_path(&mut path, &Paint::color(Color::rgba(255, 192, 0, 128)));

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(12.0);
        let _ = canvas.fill_text(x + 5.0, y + 13.0, "Frame time", &text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(14.0);
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Top);
        let _ = canvas.fill_text(x + w - 5.0, y, &format!("{:.2} FPS", 1.0 / avg), &text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 200));
        text_paint.set_font_size(12.0);
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Alphabetic);
        let _ = canvas.fill_text(
            x + w - 5.0,
            y + h - 5.0,
            &format!("{:.2} ms", avg * 1000.0),
            &text_paint,
        );
    }
}
