use std::time::Instant;

use glutin::event::{Event, WindowEvent, ElementState, KeyboardInput, VirtualKeyCode};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::WindowBuilder;
use glutin::ContextBuilder;
//use glutin::{GlRequest, Api};

use svg::node::element::path::{Command, Data, Position};
use svg::node::element::tag::{Path, Group, Type};
use svg::parser::Event as SvgEvent;

use gpucanvas::{
    Renderer,
    Canvas,
    Color,
    Paint,
    LineCap,
    LineJoin,
    FillRule,
    Winding,
    ImageFlags,
    Align,
    //CompositeOperation,
    renderer::OpenGl
};

fn main() {
    let el = EventLoop::new();
    let wb = WindowBuilder::new().with_inner_size(glutin::dpi::PhysicalSize::new(1000, 600)).with_title("gpucanvas demo");

    let windowed_context = ContextBuilder::new().with_vsync(false).build_windowed(wb, &el).unwrap();
    //let windowed_context = ContextBuilder::new().with_gl(GlRequest::Specific(Api::OpenGl, (4, 4))).with_vsync(false).build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new(|s| windowed_context.get_proc_address(s) as *const _).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    canvas.add_font("examples/assets/Roboto-Bold.ttf");
    canvas.add_font("examples/assets/Roboto-Light.ttf");
    canvas.add_font("examples/assets/Roboto-Regular.ttf");

    //let image_id = canvas.create_image_file("examples/assets/rust-logo.png", ImageFlags::GENERATE_MIPMAPS).expect("Cannot create image");

    let mut screenshot_image_id = None;

    let start = Instant::now();
    let mut prevt = start;

    let mut mousex = 0.0;
    let mut mousey = 0.0;

    let mut perf = PerfGraph::new();

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::CursorMoved { device_id: _, position, ..} => {
                    mousex = position.x as f32;
                    mousey = position.y as f32;
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::S), state: ElementState::Pressed, .. }, .. } => {
                    if let Some(screenshot_image_id) = screenshot_image_id {
                        canvas.delete_image(screenshot_image_id);
                    }

                    if let Some(image) = canvas.screenshot() {
                        screenshot_image_id = Some(canvas.create_image(&image, ImageFlags::empty()));
                    }
                }
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit
                }
                _ => (),
            }
            Event::RedrawRequested(_) => {
                let now = Instant::now();
                let dt = (now - prevt).as_secs_f32();
                prevt = now;

                perf.update(dt);

                let dpi_factor = windowed_context.window().scale_factor();
                let size = windowed_context.window().inner_size();

                let t = start.elapsed().as_secs_f32();

                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.3, 0.3, 0.32));

                let height = size.height as f32;
                let width = size.width as f32;

                let svg = svg::open("examples/assets/Ghostscript_Tiger.svg").unwrap();
                //let svg = svg::open("examples/assets/test.svg").unwrap();

                canvas.save();
                canvas.translate(canvas.width() / 2.0, canvas.height() / 2.0);

                let mut fill_paint = None;
                let mut stroke_paint = None;

                for event in svg {
                    match event {
                        SvgEvent::Tag(Group, Type::Start, attributes) => {
                            if let Some(fill) = attributes.get("fill") {
                                fill_paint = Some(Paint::color(Color::hex(fill)));
                            } else {
                                fill_paint = None;
                            }

                            if let Some(stroke) = attributes.get("fill") {
                                stroke_paint = Some(Paint::color(Color::hex(stroke)));
                            } else {
                                stroke_paint = None;
                            }

                            if let Some(stroke_width) = attributes.get("stroke-width") {
                                if let Some(stroke_paint) = stroke_paint.as_mut() {
                                    stroke_paint.set_stroke_width(stroke_width.parse().unwrap());
                                }
                            }
                        }
                        SvgEvent::Tag(Group, Type::End, _) => {
                            //dbg!("asd");
                            fill_paint = None;
                            stroke_paint = None;
                        }
                        SvgEvent::Tag(Path, _, attributes) => {
                            let data = attributes.get("d").unwrap();
                            let data = Data::parse(data).unwrap();

                            //dbg!(attributes);

                            canvas.begin_path();

                            //let mut offset_x = 0.0;
                            //let mut offset_y = 0.0;

                            let mut prevcx = 0.0;
                            let mut prevcy = 0.0;

                            for command in data.iter() {
                                //dbg!(command);
                                match command {
                                    Command::Move(pos, par) => {
                                        match pos {
                                            Position::Relative => canvas.move_to(par[0], par[1]),
                                            Position::Absolute => canvas.move_to(par[0], par[1]),
                                        }
                                        //offset_x = par[0];
                                        //offset_y = par[1];
                                    }
                                    Command::Line(pos, par) => match pos {
                                        Position::Relative => canvas.line_to(canvas.lastx() + par[0], canvas.lasty() + par[1]),
                                        Position::Absolute => canvas.line_to(par[0], par[1]),
                                    }
                                    Command::CubicCurve(pos, par) => {
                                        //dbg!(_pos);
                                        for points in par.chunks_exact(6) {
                                            match pos {
                                                Position::Relative => {
                                                    canvas.bezier_to(canvas.lastx() + points[0], canvas.lasty() + points[1], canvas.lastx() + points[2], canvas.lasty() + points[3], canvas.lastx() + points[4], canvas.lasty() + points[5]);
                                                    prevcx = points[2];
                                                    prevcy = points[3];
                                                }
                                                Position::Absolute => {
                                                    canvas.bezier_to(points[0], points[1], points[2], points[3], points[4], points[5]);
                                                    prevcx = points[2];
                                                    prevcy = points[3];
                                                }
                                            }
                                            //canvas.line_to(offset_x + points[4], offset_y + points[5]);
                                        }
                                    }
                                    Command::SmoothCubicCurve(pos, par) => {
                                        for points in par.chunks_exact(4) {
                                            let lastx = canvas.lastx();
                                            let lasty = canvas.lasty();

                                            match pos {
                                                Position::Relative => {
                                                    canvas.bezier_to(
                                                        canvas.lastx() + (2.0*lastx-prevcx),
                                                        canvas.lasty() + (2.0*lasty-prevcy),
                                                        canvas.lastx() + points[0],
                                                        canvas.lasty() + points[1],
                                                        canvas.lastx() + points[2],
                                                        canvas.lasty() + points[3]
                                                    );

                                                    prevcx = points[0];
                                                    prevcy = points[1];
                                                }
                                                Position::Absolute => {
                                                    canvas.bezier_to(2.0*lastx-prevcx, 2.0*lasty-prevcy, points[0], points[1], points[2], points[3]);
                                                    prevcx = points[0];
                                                    prevcy = points[1];
                                                }
                                            }
                                        }
                                    }
                                    Command::Close => canvas.close_path(),
                                    _ => {}
                                }
                            }



                            if let Some(stroke) = stroke_paint {
                                canvas.stroke_path(stroke);
                            }

                            if let Some(paint) = fill_paint {
                                canvas.fill_path(paint);
                            }
                        }
                        _ => {}
                    }
                }

                canvas.restore();

                //dbg!("asd");

                perf.render(&mut canvas, 5.0, 5.0);
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

        canvas.begin_path();
        canvas.rect(x, y, w, h);
        canvas.fill_path(Paint::color(Color::rgba(0, 0, 0, 128)));

        canvas.begin_path();
        canvas.move_to(x, y + h);

        for i in 0..self.history_count {
            let mut v = 1.0 / (0.00001 + self.values[(self.head+i) % self.history_count]);
			if v > 80.0 { v = 80.0; }
			let vx = x + (i as f32 / (self.history_count-1) as f32) * w;
			let vy = y + h - ((v / 80.0) * h);
			canvas.line_to(vx, vy);
        }

        canvas.line_to(x+w, y+h);
        canvas.fill_path(Paint::color(Color::rgba(255, 192, 0, 128)));

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
        text_paint.set_font_size(16);
        text_paint.set_font_name("Roboto-Regular");
    	canvas.fill_text(x + 5.0, y + 15.0, &format!("{:.2} FPS", 1.0 / avg), text_paint);

        let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 200));
        text_paint.set_font_size(14);
        text_paint.set_font_name("Roboto-Regular");
    	canvas.fill_text(x + 5.0, y + 30.0, &format!("{:.2} ms", avg * 1000.0), text_paint);
    }
}
