
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

    let window_size = glutin::dpi::PhysicalSize::new(1000, 600);
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
    canvas.add_font("examples/assets/amiri-regular.ttf");

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

                draw_baselines(&mut canvas, 5.0, 50.0, font_size as u32);
                draw_alignments(&mut canvas, 120.0, 200.0, font_size as u32);
                draw_paragraph(&mut canvas, 5.0, 380.0, font_size as u32, LOREM_TEXT);
                draw_inc_size(&mut canvas, 270.0, 30.0);
                draw_arabic(&mut canvas, 270.0, 340.0, font_size as u32);
                draw_stroked(&mut canvas, size.width as f32 - 200.0, 100.0);

                let mut paint = Paint::color(Color::hex("B7410E"));
                paint.set_font_name("Roboto-Bold");
                paint.set_text_baseline(Baseline::Top);
                paint.set_text_align(Align::Right);
                canvas.fill_text(size.width as f32 - 10.0, 10.0, format!("Scroll to increase / decrease font size. Current: {}", font_size), paint);

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
        let y = y + i as f32 * 40.0;

        let mut path = Path::new();
        path.move_to(x, y + 0.5);
        path.line_to(x + 250., y + 0.5);
        canvas.stroke_path(&mut path, Paint::color(Color::rgba(255, 32, 32, 128)));

        paint.set_text_baseline(*baseline);
        let bbox = canvas.fill_text(10.0, y, format!("AbcpKjgF Baseline::{:?}", baseline), paint);
        //let bbox = canvas.fill_text(x, y, format!("d النص العربي جميل جدا {:?}", baseline), paint);

        let mut path = Path::new();
        path.rect(bbox[0]+0.5, bbox[1]+0.5, bbox[2]+0.5 - bbox[0]+0.5, bbox[3]+0.5 - bbox[1]+0.5);
        canvas.stroke_path(&mut path, Paint::color(Color::rgba(100, 100, 100, 64)));
    }
}

fn draw_alignments<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, font_size: u32) {
    let alignments = [Align::Left, Align::Center, Align::Right];

    let mut path = Path::new();
    path.move_to(x + 0.5, y - 20.);
    path.line_to(x + 0.5, y + 80.);
    canvas.stroke_path(&mut path, Paint::color(Color::rgba(255, 32, 32, 128)));

    let mut paint = Paint::color(Color::black());
    paint.set_font_name("Roboto-Regular");
    paint.set_font_size(font_size);

    for (i, alignment) in alignments.iter().enumerate() {
        paint.set_text_align(*alignment);
        let bbox = canvas.fill_text(x, y + i as f32 * 30.0, format!("Align::{:?}", alignment), paint);

        let mut path = Path::new();
        path.rect(bbox[0]+0.5, bbox[1]+0.5, bbox[2]+0.5 - bbox[0]+0.5, bbox[3]+0.5 - bbox[1]+0.5);
        canvas.stroke_path(&mut path, Paint::color(Color::rgba(100, 100, 100, 64)));
    }
}

fn draw_paragraph<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, font_size: u32, text: &str) {

    let mut paint = Paint::color(Color::black());
    paint.set_font_name("Roboto-Light");
    paint.set_font_size(font_size);

    let mut cursor_y = y;

    for line in text.lines() {
        let bbox = canvas.fill_text(x, cursor_y, line, paint);
        cursor_y += bbox[3] - bbox[1];
    }
}

fn draw_inc_size<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32) {
    let mut cursor_y = y;

    for i in 4..24 {
        let mut paint = Paint::color(Color::black());
        paint.set_font_name("Roboto-Regular");
        paint.set_font_size(i);
        let bbox = canvas.fill_text(x, cursor_y, "The quick brown fox jumps over the lazy dog", paint);
        cursor_y += bbox[3] - bbox[1];
    }
}

fn draw_stroked<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32) {
    let mut paint = Paint::color(Color::rgba(0, 0, 0, 128));
    paint.set_font_name("Roboto-Bold");
    paint.set_stroke_width(12.0);
    paint.set_font_size(72);
    paint.set_font_blur(2.0);
    canvas.stroke_text(x, y, "RUST", paint);

    paint.set_font_blur(0.0);
    paint.set_color(Color::black());
    paint.set_stroke_width(10.0);
    canvas.stroke_text(x - 5.0, y - 5.0, "RUST", paint);

    paint.set_stroke_width(6.0);
    paint.set_color(Color::hex("#B7410E"));
    canvas.stroke_text(x - 3.0, y - 3.0, "RUST", paint);

    paint.set_color(Color::white());
    canvas.fill_text(x, y, "RUST", paint);
}

fn draw_arabic<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, font_size: u32) {
    let mut paint = Paint::color(Color::black());
    paint.set_font_name("Roboto-Regular");
    paint.set_font_size(font_size);
    
    canvas.fill_text(x, y, "Mixed latin and النص العربي جميل جدا Some more latin. Малко кирилица.", paint);
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

const LOREM_TEXT: &str = r#"
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Curabitur in nisi at ligula lobortis pretium. Sed vel eros tincidunt, fermentum metus sit amet, accumsan massa. Vestibulum sed elit et purus suscipit
suscipit nec ac augue. Duis elit nisi, porttitor porta est sed, blandit ultricies odio. Morbi faucibus sagittis justo in accumsan. Proin quis felis hendrerit, egestas ligula ut, pellentesque nibh.
Sed at gravida lectus. Duis eu nisl non sem lobortis rutrum. Sed non mauris urna. Pellentesque suscipit nec odio eu varius. Quisque lobortis elit in finibus vulputate. Mauris quis gravida libero.
Etiam non malesuada felis, nec fringilla quam.

Donec vitae dignissim tellus. Morbi lobortis finibus purus non porttitor. In mi enim, lacinia et condimentum ut, venenatis nec magna. Sed id ex in metus vulputate facilisis sit amet in arcu.
Fusce tempus, mauris non porta ultricies, velit nulla blandit diam, vel maximus metus mi sed erat. Nam hendrerit enim sit amet nisl dictum gravida. Mauris faucibus feugiat neque ac interdum.
In dignissim orci id diam suscipit, id interdum nunc aliquam. Nam auctor, neque sit amet molestie euismod, dolor nibh pulvinar ligula, at sagittis nisi dolor a nisl. Praesent placerat ut enim
tincidunt rhoncus. Cras vel feugiat leo. Donec arcu metus, placerat non est eget, laoreet dapibus odio. In ac massa et lectus tempus imperdiet.

Phasellus lobortis gravida turpis non auctor. Nam euismod consectetur imperdiet. Nunc egestas ultricies bibendum. Donec consequat purus quis tempus aliquam. Suspendisse mauris nunc, dignissim placerat
accumsan at, cursus sit amet mi. In faucibus ac neque non hendrerit. Maecenas in sollicitudin nibh.

In hac habitasse platea dictumst. Pellentesque at libero quis diam interdum elementum vel sed tortor. Etiam eget urna pretium, euismod orci vel, convallis arcu. Curabitur a ipsum in neque molestie finibus.
Phasellus mollis volutpat massa non gravida. Duis vel libero mollis, mollis eros vitae, ornare dolor. Morbi interdum, tellus et pulvinar pharetra, justo ante accumsan neque, quis venenatis augue ipsum sed
odio. Mauris sit amet lectus et nisl faucibus interdum. Nunc dapibus quis odio ac dictum. In iaculis nibh est, sit amet malesuada mi eleifend ut. Nunc dignissim tempor sollicitudin. Nulla facilisi.
Nullam dictum, tortor in elementum malesuada, purus lectus placerat ipsum, vitae pellentesque risus ante sed turpis.

In ac dictum metus. Phasellus fermentum ac tortor id gravida. Quisque vitae dui velit. Vestibulum rutrum bibendum aliquam. Sed vitae pretium nisi, quis mattis justo. Ut dolor massa, suscipit sed
condimentum vitae, finibus in nisi. Aliquam posuere nulla leo, sit amet rhoncus lorem vulputate id. Phasellus imperdiet ultricies est a aliquet. Curabitur vehicula porta posuere. Duis eget purus
condimentum, elementum odio sit amet, convallis tellus.
"#;
