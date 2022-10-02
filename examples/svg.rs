use std::time::Instant;

use femtovg::{renderer::OpenGl, Align, Baseline, Canvas, Color, FillRule, FontId, ImageFlags, Paint, Path, Renderer};
use glutin::{
    event::{ElementState, Event, KeyboardInput, MouseButton, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    ContextBuilder,
};

fn main() {
    let el = EventLoop::new();
    let wb = WindowBuilder::new()
        .with_inner_size(glutin::dpi::PhysicalSize::new(1000, 600))
        .with_title("femtovg demo");

    let windowed_context = ContextBuilder::new().with_vsync(false).build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new_from_glutin_context(&windowed_context).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    let roboto_light = canvas
        .add_font("examples/assets/Roboto-Light.ttf")
        .expect("Cannot add font");

    let roboto_regular = canvas
        .add_font("examples/assets/Roboto-Regular.ttf")
        .expect("Cannot add font");

    let mut screenshot_image_id = None;

    let start = Instant::now();
    let mut prevt = start;

    let mut mousex = 0.0;
    let mut mousey = 0.0;
    let mut dragging = false;

    let mut perf = PerfGraph::new();

    let svg_data = std::fs::read("examples/assets/Ghostscript_Tiger.svg").unwrap();
    let tree = usvg::Tree::from_data(&svg_data, &usvg::Options::default().to_ref()).unwrap();

    let mut paths = render_svg(tree);

    // print memory usage
    let mut total_sisze_bytes = 0;

    for path in &paths {
        total_sisze_bytes += path.0.size();
    }

    println!("Path mem usage: {}kb", total_sisze_bytes / 1024);

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::MouseInput {
                    button: MouseButton::Left,
                    state,
                    ..
                } => match state {
                    ElementState::Pressed => dragging = true,
                    ElementState::Released => dragging = false,
                },
                WindowEvent::CursorMoved {
                    device_id: _, position, ..
                } => {
                    if dragging {
                        let p0 = canvas.transform().inversed().transform_point(mousex, mousey);
                        let p1 = canvas
                            .transform()
                            .inversed()
                            .transform_point(position.x as f32, position.y as f32);

                        canvas.translate(p1.0 - p0.0, p1.1 - p0.1);
                    }

                    mousex = position.x as f32;
                    mousey = position.y as f32;
                }
                WindowEvent::MouseWheel {
                    device_id: _, delta, ..
                } => match delta {
                    glutin::event::MouseScrollDelta::LineDelta(_, y) => {
                        let pt = canvas.transform().inversed().transform_point(mousex, mousey);
                        canvas.translate(pt.0, pt.1);
                        canvas.scale(1.0 + (y / 10.0), 1.0 + (y / 10.0));
                        canvas.translate(-pt.0, -pt.1);
                    }
                    _ => (),
                },
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            virtual_keycode: Some(VirtualKeyCode::S),
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } => {
                    if let Some(screenshot_image_id) = screenshot_image_id {
                        canvas.delete_image(screenshot_image_id);
                    }

                    if let Ok(image) = canvas.screenshot() {
                        screenshot_image_id = Some(canvas.create_image(image.as_ref(), ImageFlags::empty()).unwrap());
                    }
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => (),
            },
            Event::RedrawRequested(_) => {
                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                perf.update(dt);

                let dpi_factor = windowed_context.window().scale_factor();
                let size = windowed_context.window().inner_size();

                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.3, 0.3, 0.32));

                canvas.save();
                canvas.translate(200.0, 200.0);

                for (path, fill, stroke) in &mut paths {
                    if let Some(fill) = fill {
                        fill.set_anti_alias(true);
                        canvas.fill_path(path, *fill);
                    }

                    if let Some(stroke) = stroke {
                        stroke.set_anti_alias(true);
                        canvas.stroke_path(path, *stroke);
                    }

                    if canvas.contains_point(path, mousex, mousey, FillRule::NonZero) {
                        let mut paint = Paint::color(Color::rgb(32, 240, 32));
                        paint.set_line_width(1.0);
                        canvas.stroke_path(path, paint);
                    }
                }

                canvas.restore();

                canvas.save();
                canvas.reset();
                perf.render(&mut canvas, roboto_regular, roboto_light, 5.0, 5.0);
                canvas.restore();

                canvas.flush();
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => windowed_context.window().request_redraw(),
            _ => (),
        }
    });
}

fn render_svg(svg: usvg::Tree) -> Vec<(Path, Option<Paint>, Option<Paint>)> {
    use usvg::NodeKind;
    use usvg::PathSegment;

    let mut paths = Vec::new();

    for node in svg.root().descendants() {
        match &*node.borrow() {
            NodeKind::Path(svg_path) => {
                let mut path = Path::new();

                for command in svg_path.data.iter() {
                    match command {
                        PathSegment::MoveTo { x, y } => path.move_to(*x as f32, *y as f32),
                        PathSegment::LineTo { x, y } => path.line_to(*x as f32, *y as f32),
                        PathSegment::CurveTo { x1, y1, x2, y2, x, y } => {
                            path.bezier_to(*x1 as f32, *y1 as f32, *x2 as f32, *y2 as f32, *x as f32, *y as f32)
                        }
                        PathSegment::ClosePath => path.close(),
                    }
                }

                let to_femto_color = |usvg_paint: &usvg::Paint| match usvg_paint {
                    usvg::Paint::Color(usvg::Color { red, green, blue }) => Some(Color::rgb(*red, *green, *blue)),
                    _ => None,
                };

                let fill = svg_path
                    .fill
                    .as_ref()
                    .and_then(|fill| to_femto_color(&fill.paint))
                    .map(Paint::color);

                let stroke = svg_path.stroke.as_ref().and_then(|stroke| {
                    to_femto_color(&stroke.paint).map(|paint| {
                        let mut stroke_paint = Paint::color(paint);
                        stroke_paint.set_line_width(stroke.width.value() as f32);
                        stroke_paint
                    })
                });

                paths.push((path, fill, stroke))
            }
            _ => (),
        }
    }

    paths
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

    fn render<T: Renderer>(&self, canvas: &mut Canvas<T>, regular_font: FontId, light_font: FontId, x: f32, y: f32) {
        let avg = self.get_average();

        let w = 200.0;
        let h = 35.0;

        let mut path = Path::new();
        path.rect(x, y, w, h);
        //canvas.fill_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 128)));

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
        canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 192, 0, 128)));

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(12.0);
        text_paint.set_font(&[light_font]);
        let _ = canvas.fill_text(x + 5.0, y + 13.0, "Frame time", text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(14.0);
        text_paint.set_font(&[regular_font]);
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Top);
        let _ = canvas.fill_text(x + w - 5.0, y, &format!("{:.2} FPS", 1.0 / avg), text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 200));
        text_paint.set_font_size(12.0);
        text_paint.set_font(&[light_font]);
        text_paint.set_text_align(Align::Right);
        text_paint.set_text_baseline(Baseline::Alphabetic);
        let _ = canvas.fill_text(x + w - 5.0, y + h - 5.0, &format!("{:.2} ms", avg * 1000.0), text_paint);
    }
}
