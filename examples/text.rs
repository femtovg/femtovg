
use std::time::Instant;

use glutin::event::{Event, WindowEvent, DeviceEvent, ElementState, KeyboardInput, VirtualKeyCode, MouseButton};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::{Window, WindowBuilder};
use glutin::ContextBuilder;

use gpucanvas::{
    Renderer,
    Canvas,
    Color,
    Paint,
    ImageFlags,
    Align,
    Baseline,
    ImageId,
    Path,
    //CompositeOperation,
    renderer::OpenGl
};

fn main() {

    let window_size = glutin::dpi::PhysicalSize::new(800, 600);
    let el = EventLoop::new();
    let wb = WindowBuilder::new()
        .with_inner_size(window_size)
        .with_resizable(false)
        .with_title("Text demo");

    let windowed_context = ContextBuilder::new().build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new(|s| windowed_context.get_proc_address(s) as *const _).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    canvas.set_size(window_size.width as u32, window_size.height as u32, windowed_context.window().scale_factor() as f32);

    canvas.add_font("examples/assets/Roboto-Bold.ttf");
    canvas.add_font("examples/assets/Roboto-Light.ttf");
    canvas.add_font("examples/assets/Roboto-Regular.ttf");

    let start = Instant::now();
    let mut prevt = start;

    let mut perf = PerfGraph::new();

    let mut font_size = 18;

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit
                }
                WindowEvent::MouseWheel { device_id: _, delta, .. } => match delta {
                    glutin::event::MouseScrollDelta::LineDelta(_, y) => {
                        font_size += *y as i32;
                    },
                    _ => ()
                }
                _ => (),
            }
            Event::RedrawRequested(_) => {
                let dpi_factor = windowed_context.window().scale_factor();
                let size = windowed_context.window().inner_size();
                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.9, 0.9, 0.9));

                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                perf.update(dt);

                // let y = 100.5;
                // let mut path = Path::new();
                // path.move_to(10.0, y);
                // path.line_to(310.0, y);
                // canvas.stroke_path(&mut path, Paint::color(Color::rgb(255, 32, 32)));
                //
                // let mut paint = Paint::color(Color::black());
                // paint.set_font_name("Roboto-Regular");
                // paint.set_font_size(18);
                // paint.set_text_baseline(Baseline::Top);
                // canvas.fill_text(10.0, y, "Top", paint);
                // paint.set_text_baseline(Baseline::Middle);
                // canvas.fill_text(50.0, y, "Middle", paint);
                // paint.set_text_baseline(Baseline::Alphabetic);
                // canvas.fill_text(120.0, y, "Alphabetic", paint);

                draw_baselines(&mut canvas, 5.0, 50.0, font_size as u32);

                canvas.save();
                canvas.reset();
                perf.render(&mut canvas, 5.0, 5.0);
                canvas.restore();

                canvas.flush();
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => {
                windowed_context.window().request_redraw()
            }
            _ => (),
        }
    });
}

fn draw_baselines<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, font_size: u32) {
    let baselines = [Baseline::Top, Baseline::Middle, Baseline::Alphabetic, Baseline::Bottom];

    let mut paint = Paint::color(Color::black());
    paint.set_font_name("Roboto-Regular");
    paint.set_font_size(font_size);

    for (i, baseline) in baselines.iter().enumerate() {
        let y = y + i as f32 * 50.0;

        let mut path = Path::new();
        path.move_to(x, y + 0.5);
        path.line_to(x + 250., y + 0.5);
        canvas.stroke_path(&mut path, Paint::color(Color::rgb(255, 32, 32)));

        paint.set_text_baseline(*baseline);
        let bbox = canvas.fill_text(10.0, y, format!("AbcpKjgF baseline ({:?})", baseline), paint);

        let mut path = Path::new();
        path.rect(bbox[0]+0.5, bbox[1]+0.5, bbox[2]+0.5 - bbox[0]+0.5, bbox[3]+0.5 - bbox[1]+0.5);
        canvas.stroke_path(&mut path, Paint::color(Color::rgba(255, 32, 32, 128)));
    }
}

struct PerfGraph {
    history_count: usize,
    values: Vec<f32>,
    head: usize
}

impl PerfGraph {
    fn new() -> Self {
        Self {
            history_count: 100,
            values: vec![0.0; 100],
            head: Default::default()
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
        canvas.fill_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 128)));

        let mut path = Path::new();
        path.move_to(x, y + h);

        for i in 0..self.history_count {
            let mut v = 1.0 / (0.00001 + self.values[(self.head+i) % self.history_count]);
			if v > 80.0 { v = 80.0; }
			let vx = x + (i as f32 / (self.history_count-1) as f32) * w;
			let vy = y + h - ((v / 80.0) * h);
			path.line_to(vx, vy);
        }

        path.line_to(x+w, y+h);
        canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 192, 0, 128)));

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(12);
        text_paint.set_font_name("Roboto-Light");
    	canvas.fill_text(x + 5.0, y + 13.0, "Frame time", text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(14);
        text_paint.set_font_name("Roboto-Regular");
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Top);
    	canvas.fill_text(x + w - 5.0, y + 2., &format!("{:.2} FPS", 1.0 / avg), text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 200));
        text_paint.set_font_size(12);
        text_paint.set_font_name("Roboto-Light");
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Alphabetic);
    	canvas.fill_text(x + w - 5.0, y + h - 5.0, &format!("{:.2} ms", avg * 1000.0), text_paint);
    }
}
