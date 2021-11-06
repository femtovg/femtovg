use std::time::Instant;

use glutin::{
    event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    ContextBuilder,
};

use femtovg::{renderer::OpenGl, Align, Baseline, Canvas, Color, FontId, ImageFlags, ImageId, Paint, Path, Renderer};

struct Fonts {
    sans: FontId,
    bold: FontId,
    light: FontId,
}

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
    canvas.set_size(
        window_size.width as u32,
        window_size.height as u32,
        windowed_context.window().scale_factor() as f32,
    );

    let fonts = Fonts {
        sans: canvas
            .add_font("examples/assets/Roboto-Regular.ttf")
            .expect("Cannot add font"),
        bold: canvas
            .add_font("examples/assets/Roboto-Bold.ttf")
            .expect("Cannot add font"),
        light: canvas
            .add_font("examples/assets/Roboto-Light.ttf")
            .expect("Cannot add font"),
    };

    // The fact that a font is added to the canvas is enough for it to be considered when
    // searching for fallbacks
    let _ = canvas.add_font("examples/assets/amiri-regular.ttf");

    let supports_emojis = canvas.add_font("/System/Library/Fonts/Apple Color Emoji.ttc").is_ok();

    let flags = ImageFlags::GENERATE_MIPMAPS | ImageFlags::REPEAT_X | ImageFlags::REPEAT_Y;
    let image_id = canvas
        .load_image_file("examples/assets/pattern.jpg", flags)
        .expect("Cannot create image");

    let start = Instant::now();
    let mut prevt = start;

    let mut perf = PerfGraph::new();

    let mut font_size = 18.0;

    #[cfg(feature = "debug_inspector")]
    let mut font_texture_to_show: Option<usize> = None;

    let mut x = 5.0;
    let mut y = 380.0;

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            virtual_keycode: Some(keycode),
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } => {
                    if *keycode == VirtualKeyCode::W {
                        y -= 0.1;
                    }

                    if *keycode == VirtualKeyCode::S {
                        y += 0.1;
                    }

                    if *keycode == VirtualKeyCode::A {
                        x -= 0.1;
                    }

                    if *keycode == VirtualKeyCode::D {
                        x += 0.1;
                    }

                    if *keycode == VirtualKeyCode::NumpadAdd {
                        font_size += 1.0;
                    }

                    if *keycode == VirtualKeyCode::NumpadSubtract {
                        font_size -= 1.0;
                    }
                }
                #[cfg(feature = "debug_inspector")]
                WindowEvent::MouseInput {
                    device_id: _,
                    state: ElementState::Pressed,
                    ..
                } => {
                    let len = canvas.debug_inspector_get_font_textures().len();
                    let next = match font_texture_to_show {
                        None => 0,
                        Some(i) => i + 1,
                    };
                    font_texture_to_show = if next < len { Some(next) } else { None };
                }
                WindowEvent::MouseWheel {
                    device_id: _, delta, ..
                } => match delta {
                    glutin::event::MouseScrollDelta::LineDelta(_, y) => {
                        font_size = font_size + *y / 2.0;
                        font_size = font_size.max(2.0);
                    }
                    _ => (),
                },
                _ => (),
            },
            Event::RedrawRequested(_) => {
                let dpi_factor = windowed_context.window().scale_factor();
                let size = windowed_context.window().inner_size();
                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.9, 0.9, 0.9));

                let elapsed = start.elapsed().as_secs_f32();
                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                perf.update(dt);

                draw_baselines(&mut canvas, &fonts, 5.0, 50.0, font_size, supports_emojis);
                draw_alignments(&mut canvas, &fonts, 120.0, 200.0, font_size);
                draw_paragraph(&mut canvas, &fonts, x, y, font_size, LOREM_TEXT);
                draw_inc_size(&mut canvas, &fonts, 300.0, 10.0);

                draw_complex(&mut canvas, 300.0, 340.0, font_size);

                draw_stroked(&mut canvas, &fonts, size.width as f32 - 200.0, 100.0);
                draw_gradient_fill(&mut canvas, &fonts, size.width as f32 - 200.0, 180.0);
                draw_image_fill(&mut canvas, &fonts, size.width as f32 - 200.0, 260.0, image_id, elapsed);

                let mut paint = Paint::color(Color::hex("B7410E"));
                paint.set_font(&[fonts.bold]);
                paint.set_text_baseline(Baseline::Top);
                paint.set_text_align(Align::Right);
                let _ = canvas.fill_text(
                    size.width as f32 - 10.0,
                    10.0,
                    format!("Scroll to increase / decrease font size. Current: {}", font_size),
                    paint,
                );
                #[cfg(feature = "debug_inspector")]
                let _ = canvas.fill_text(
                    size.width as f32 - 10.0,
                    24.0,
                    format!("Click to show font atlas texture. Current: {:?}", font_texture_to_show),
                    paint,
                );

                canvas.save();
                canvas.reset();
                perf.render(&mut canvas, 5.0, 5.0);
                canvas.restore();

                #[cfg(feature = "debug_inspector")]
                if let Some(index) = font_texture_to_show {
                    canvas.save();
                    canvas.reset();
                    let textures = canvas.debug_inspector_get_font_textures();
                    if let Some(&id) = textures.get(index) {
                        canvas.debug_inspector_draw_image(id);
                    }
                    canvas.restore();
                }

                canvas.flush();
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => windowed_context.window().request_redraw(),
            _ => (),
        }
    });
}

fn draw_baselines<T: Renderer>(
    canvas: &mut Canvas<T>,
    fonts: &Fonts,
    x: f32,
    y: f32,
    font_size: f32,
    supports_emojis: bool,
) {
    let baselines = [Baseline::Top, Baseline::Middle, Baseline::Alphabetic, Baseline::Bottom];

    let mut paint = Paint::color(Color::black());
    paint.set_font(&[fonts.sans]);
    paint.set_font_size(font_size);

    let mut base_text = "AbcpKjgF".to_string();
    if supports_emojis {
        base_text.push_str("🚀🌳");
    }

    for (i, baseline) in baselines.iter().enumerate() {
        let y = y + i as f32 * 40.0;

        let mut path = Path::new();
        path.move_to([x, y + 0.5]);
        path.line_to([x + 250., y + 0.5]);
        canvas.stroke_path(&mut path, Paint::color(Color::rgba(255, 32, 32, 128)));

        paint.set_text_baseline(*baseline);

        if let Ok(res) = canvas.fill_text(x, y, format!("{} Baseline::{:?}", base_text, baseline), paint) {
            //let res = canvas.fill_text(10.0, y, format!("d النص العربي جميل جدا {:?}", baseline), paint);

            let mut path = Path::new();
            path.rect(res.pos, res.width(), res.height());
            canvas.stroke_path(&mut path, Paint::color(Color::rgba(100, 100, 100, 64)));
        }
    }
}

fn draw_alignments<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, x: f32, y: f32, font_size: f32) {
    let alignments = [Align::Left, Align::Center, Align::Right];

    let mut path = Path::new();
    path.move_to([x + 0.5, y - 20.]);
    path.line_to([x + 0.5, y + 80.]);
    canvas.stroke_path(&mut path, Paint::color(Color::rgba(255, 32, 32, 128)));

    let mut paint = Paint::color(Color::black());
    paint.set_font(&[fonts.sans]);
    paint.set_font_size(font_size);

    for (i, alignment) in alignments.iter().enumerate() {
        paint.set_text_align(*alignment);

        if let Ok(res) = canvas.fill_text(x, y + i as f32 * 30.0, format!("Align::{:?}", alignment), paint) {
            let mut path = Path::new();
            path.rect(res.pos, res.width(), res.height());
            canvas.stroke_path(&mut path, Paint::color(Color::rgba(100, 100, 100, 64)));
        }
    }
}

fn draw_paragraph<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, x: f32, y: f32, font_size: f32, text: &str) {
    let mut paint = Paint::color(Color::black());
    paint.set_font(&[fonts.light]);
    //paint.set_text_align(Align::Right);
    paint.set_font_size(font_size);

    let font_metrics = canvas.measure_font(paint).expect("Error measuring font");

    let width = canvas.width();
    let mut y = y;

    let lines = canvas
        .break_text_vec(width, text, paint)
        .expect("Error while breaking text");

    for line_range in lines {
        if let Ok(_res) = canvas.fill_text(x, y, &text[line_range], paint) {
            y += font_metrics.height();
        }
    }

    // let mut start = 0;

    // while start < text.len() {
    //     let substr = &text[start..];

    //     if let Ok(index) = canvas.break_text(width, substr, paint) {
    //         if let Ok(res) = canvas.fill_text(x, y, &substr[0..index], paint) {
    //             y += res.height;
    //         }

    //         start += &substr[0..index].len();
    //     } else {
    //         break;
    //     }
    // }
}

fn draw_inc_size<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, x: f32, y: f32) {
    let mut cursor_y = y;

    for i in 4..23 {
        let mut paint = Paint::color(Color::black());
        paint.set_font(&[fonts.sans]);
        paint.set_font_size(i as f32);

        let font_metrics = canvas.measure_font(paint).expect("Error measuring font");

        if let Ok(_res) = canvas.fill_text(x, cursor_y, "The quick brown fox jumps over the lazy dog", paint) {
            cursor_y += font_metrics.height();
        }
    }
}

fn draw_stroked<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, x: f32, y: f32) {
    let mut paint = Paint::color(Color::rgba(0, 0, 0, 128));
    paint.set_font(&[fonts.bold]);
    paint.set_line_width(12.0);
    paint.set_font_size(72.0);
    let _ = canvas.stroke_text(x + 5.0, y + 5.0, "RUST", paint);

    paint.set_color(Color::black());
    paint.set_line_width(10.0);
    let _ = canvas.stroke_text(x, y, "RUST", paint);

    paint.set_line_width(6.0);
    paint.set_color(Color::hex("#B7410E"));
    let _ = canvas.stroke_text(x, y, "RUST", paint);

    paint.set_color(Color::white());
    let _ = canvas.fill_text(x, y, "RUST", paint);
}

fn draw_gradient_fill<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, x: f32, y: f32) {
    let mut paint = Paint::color(Color::rgba(0, 0, 0, 255));
    paint.set_font(&[fonts.bold]);
    paint.set_line_width(6.0);
    paint.set_font_size(72.0);
    let _ = canvas.stroke_text(x, y, "RUST", paint);

    let mut paint = Paint::linear_gradient(
        x,
        y - 60.0,
        x,
        y,
        Color::rgba(225, 133, 82, 255),
        Color::rgba(93, 55, 70, 255),
    );
    paint.set_font(&[fonts.bold]);
    paint.set_font_size(72.0);
    let _ = canvas.fill_text(x, y, "RUST", paint);
}

fn draw_image_fill<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, x: f32, y: f32, image_id: ImageId, t: f32) {
    let mut paint = Paint::color(Color::hex("#7300AB"));
    paint.set_line_width(3.0);
    let mut path = Path::new();
    path.move_to([x, y - 2.0]);
    path.line_to([x + 180.0, y - 2.0]);
    canvas.stroke_path(&mut path, paint);

    let text = "RUST";

    let mut paint = Paint::color(Color::rgba(0, 0, 0, 128));
    paint.set_font(&[fonts.bold]);
    paint.set_line_width(4.0);
    paint.set_font_size(72.0);
    let _ = canvas.stroke_text(x, y, text, paint);

    let mut paint = Paint::image(image_id, x, y - t * 10.0, 120.0, 120.0, 0.0, 0.50);
    //let mut paint = Paint::image(image_id, x + 50.0, y - t*10.0, 120.0, 120.0, t.sin() / 10.0, 0.70);
    paint.set_font(&[fonts.bold]);
    paint.set_font_size(72.0);
    let _ = canvas.fill_text(x, y, text, paint);
}

fn draw_complex<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, font_size: f32) {
    let mut paint = Paint::color(Color::rgb(34, 34, 34));
    paint.set_font_size(font_size);

    let _ = canvas.fill_text(
        x,
        y,
        "Latin اللغة العربية Кирилица тест iiiiiiiiiiiiiiiiiiiiiiiiiiiii\nasdasd",
        paint,
    );
    //let _ = canvas.fill_text(x, y, "اللغة العربية", paint);
    //canvas.fill_text(x, y, "Traditionally, text is composed to create a readable, coherent, and visually satisfying", paint);
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
        path.rect([x, y], w, h);
        canvas.fill_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 128)));

        let mut path = Path::new();
        path.move_to([x, y + h]);

        for i in 0..self.history_count {
            let mut v = 1.0 / (0.00001 + self.values[(self.head + i) % self.history_count]);
            if v > 80.0 {
                v = 80.0;
            }
            let vx = x + (i as f32 / (self.history_count - 1) as f32) * w;
            let vy = y + h - ((v / 80.0) * h);
            path.line_to([vx, vy]);
        }

        path.line_to([x + w, y + h]);
        canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 192, 0, 128)));

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(12.0);
        let _ = canvas.fill_text(x + 5.0, y + 13.0, "Frame time", text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(14.0);
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Top);
        let _ = canvas.fill_text(x + w - 5.0, y, &format!("{:.2} FPS", 1.0 / avg), text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 200));
        text_paint.set_font_size(12.0);
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Alphabetic);
        let _ = canvas.fill_text(x + w - 5.0, y + h - 5.0, &format!("{:.2} ms", avg * 1000.0), text_paint);
    }
}

const LOREM_TEXT: &str = r#"
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
"#;

// const LOREM_TEXT: &str = r#"
// مرئية وساهلة قراءة وجاذبة. ترتيب الحوف يشمل كل من اختيار (asdasdasdasdasdasd) عائلة الخط وحجم وطول الخط والمسافة بين السطور
// "#;
