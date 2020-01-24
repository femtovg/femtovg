
use std::time::Instant;

use glutin::event::{Event, WindowEvent, ElementState, KeyboardInput, VirtualKeyCode};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::WindowBuilder;
use glutin::ContextBuilder;
//use glutin::{GlRequest, Api};

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

    //.with_multisampling(16)
    let windowed_context = ContextBuilder::new().with_vsync(false).build_windowed(wb, &el).unwrap();
    //let windowed_context = ContextBuilder::new().with_gl(GlRequest::Specific(Api::OpenGl, (4, 4))).with_vsync(false).build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new(|s| windowed_context.get_proc_address(s) as *const _).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    canvas.add_font("examples/assets/Roboto-Bold.ttf");
    canvas.add_font("examples/assets/Roboto-Light.ttf");
    canvas.add_font("examples/assets/Roboto-Regular.ttf");
    canvas.add_font("/usr/share/fonts/noto/NotoSansArabic-Regular.ttf");

    let image_id = canvas.create_image_file("examples/assets/rust-logo.png", ImageFlags::GENERATE_MIPMAPS).expect("Cannot create image");

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
                WindowEvent::MouseWheel { device_id: _, delta, .. } => match delta {
                    glutin::event::MouseScrollDelta::LineDelta(_, y) => {
                        let pt = canvas.transformed_point(mousex, mousey);
                        canvas.translate(pt.0, pt.1);
                        canvas.scale(1.0 + (y / 10.0), 1.0 + (y / 10.0));
                        canvas.translate(-pt.0, -pt.1);
                    },
                    _ => ()
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
                //canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.3, 0.3, 0.32));
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.95, 0.95, 0.95));



                let height = size.height as f32;
                let width = size.width as f32;

                // draw_eyes(&mut canvas, width - 250.0, 50.0, 150.0, 100.0, mousex, mousey, t);
                // draw_graph(&mut canvas, 0.0, height / 2.0, width, height / 2.0, t);
                // draw_lines(&mut canvas, 120.0, height - 50.0, 600.0, 50.0, t);
                // draw_window(&mut canvas, "Widgets `n Stuff", 50.0, 50.0, 300.0, 400.0);
                //
                // draw_fills(&mut canvas, width - 200.0, height - 100.0);

                canvas.save();
                canvas.reset();
                perf.render(&mut canvas, 5.0, 5.0);
                canvas.restore();
                /*
                draw_spinner(&mut canvas, 15.0, 285.0, 10.0, t);
                draw_rects(&mut canvas, 15.0, 15.0);
                draw_caps(&mut canvas, 15.0, 110.0);
                draw_joins(&mut canvas, 110.0, 110.0);
                draw_lines(&mut canvas, 205.0, 110.0);
                draw_shadows(&mut canvas);
                */
                //draw_state_stack(&mut canvas);

                let text = "
æPadscanvas.scale(1.0,-1.0);AAQQQQQQQQQQQu:OWQIPQWERTYUIkcmlknsdkjfhweiuqhpi15646546/#$OWQIPQWERTYUIkcmlknsdkjfhwei Cursive Joining: اللغة العربي
";
                canvas.save();
                canvas.translate(0.0, 50.0);

                for line in text.lines() {
                    let mut paint = Paint::color(Color::hex("454545"));
                    paint.set_font_size(12);
                    paint.set_font_name("Roboto-Regular");
                    canvas.fill_text(10.0, 0.0, line, paint);

                    canvas.translate(0.0, 14.0);
                }

                canvas.restore();

                if false {

                    let combination_marks = format!("Comb. marks: {}{} {}{}", '\u{0061}', '\u{0300}', '\u{0061}', '\u{0328}');
                    let cursive_joining = format!("Cursive Joining: اللغة العربية");
                    let text = format!("Latin text. Ligatures æ fi ﬁ. Kerning VA Wavy. ZWJ? {} {}", combination_marks, cursive_joining);
                    //let text = format!("Morbi tincidunt pretium dolor, eu mollis augue tristique quis. Nunc tristique vulputate sem a laoreet. Etiris diam felis, laoreet sit amet nisi eu, pulvinar facilisis massa. ");

                    //let bounds = canvas.text_bounds(15.0, 300.0, text);

                    let mut paint = Paint::color(Color::hex("454545"));

                    let font_size = 16;

                    paint.set_stroke_width(1.0);
                    paint.set_font_size(font_size);
                    //paint.set_letter_spacing(3);
                    //paint.set_font_blur(1.0);
                    //paint.set_font_name("BitstreamVeraSerif-Roman".to_string());
                    paint.set_font_name("NotoSans-Regular");

                    canvas.fill_text(15.0, 220.0, &text, paint);
                    //canvas.stroke_text(15.0 + x, y + 10.0 + font_size as f32, &line, &paint);
                }

                if let Some(image_id) = screenshot_image_id {
                    let x = size.width as f32 - 512.0;
                    let y = size.height as f32 - 512.0;

                    let paint = Paint::image(image_id, x, y, 512.0, 512.0, 0.0, 1.0);

                    canvas.begin_path();
                    canvas.rect(x, y, 512.0, 512.0);
                    canvas.fill_path(paint);
                    canvas.stroke_path(Paint::color(Color::hex("454545")));
                }

                // Image
                if false {
                    canvas.save();
                    canvas.translate(490.0, 110.0);

                    let paint = Paint::image(image_id, 0.0, 0.0, 80.0, 80.0, 0.0, 1.0);

                    canvas.begin_path();
                    canvas.rect(10.0, 10.0, 512.0, 512.0);
                    canvas.fill_path(paint);

                    canvas.restore();
                }

                /*
                let elapsed = cpu_start.elapsed().as_secs_f32();

                canvas.fill_text(15.0, size.height as f32 - 45.0, &format!("CPU Time: {}", elapsed), &Paint::color(Color::hex("454545")));

                canvas.begin_path();
                canvas.rect(15.0, size.height as f32 - 40.0, 200.0*(elapsed / 0.016), 3.0);
                canvas.fill_path(Paint::color(Color::hex("000000")));
                canvas.begin_path();
                canvas.rect(15.0, size.height as f32 - 40.0, 200.0, 3.0);
                canvas.stroke_path(Paint::color(Color::hex("bababa")));

                let gpu_time = Instant::now();

                canvas.end_frame();

                canvas.fill_text(15.0, size.height as f32 - 20.0, &format!("GPU Time: {:?}", gpu_time.elapsed()), &Paint::color(Color::hex("454545")));
                */

                canvas.flush();
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => {
                //scroll = 1.0;
                windowed_context.window().request_redraw()
            }
            _ => (),
        }
    });
}

fn draw_eyes<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, h: f32, mx: f32, my: f32, t: f32) {
    let ex = w *0.23;
    let ey = h * 0.5;
    let lx = x + ex;
    let ly = y + ey;
    let rx = x + w - ex;
    let ry = y + ey;
    let br = if ex < ey { ex } else { ey } * 0.5;
    let blink = 1.0 - (t*0.5).sin().powf(200.0)*0.8;

    let bg = Paint::linear_gradient(x, y + h * 0.5, x + w * 0.1, y + h, Color::rgba(0,0,0,32), Color::rgba(0,0,0,16));
	canvas.begin_path();
	canvas.ellipse(lx + 3.0, ly + 16.0, ex, ey);
	canvas.ellipse(rx + 3.0, ry + 16.0, ex, ey);
	canvas.fill_path(bg);

	let bg = Paint::linear_gradient(x, y + h * 0.25, x + w * 0.1, y + h, Color::rgba(220,220,220,255), Color::rgba(128,128,128,255));
	canvas.begin_path();
	canvas.ellipse(lx, ly, ex, ey);
	canvas.ellipse(rx, ry, ex, ey);
	canvas.fill_path(bg);

	let mut dx = (mx - rx) / (ex * 10.0);
	let mut dy = (my - ry) / (ey * 10.0);
	let d = (dx*dx+dy*dy).sqrt();
	if d > 1.0 {
		dx /= d; dy /= d;
	}

	dx *= ex*0.4;
	dy *= ey*0.5;
	canvas.begin_path();
	canvas.ellipse(lx + dx, ly + dy + ey * 0.25 * (1.0 - blink), br, br * blink);
	canvas.fill_path(Paint::color(Color::rgba(32,32,32,255)));

	let mut dx = (mx - rx) / (ex * 10.0);
	let mut dy = (my - ry) / (ey * 10.0);
	let d = (dx*dx+dy*dy).sqrt();
	if d > 1.0 {
		dx /= d; dy /= d;
	}

	dx *= ex*0.4;
	dy *= ey*0.5;
	canvas.begin_path();
	canvas.ellipse(rx + dx, ry + dy + ey * 0.25 * (1.0 - blink), br, br*blink);
	canvas.fill_path(Paint::color(Color::rgba(32,32,32,255)));

	let gloss = Paint::radial_gradient(lx - ex * 0.25, ly - ey * 0.5, ex * 0.1, ex * 0.75, Color::rgba(255,255,255,128), Color::rgba(255,255,255,0));
	canvas.begin_path();
	canvas.ellipse(lx,ly, ex,ey);
	canvas.fill_path(gloss);

	let gloss = Paint::radial_gradient(rx - ex * 0.25, ry - ey * 0.5, ex * 0.1, ex * 0.75, Color::rgba(255,255,255,128), Color::rgba(255,255,255,0));
	canvas.begin_path();
	canvas.ellipse(rx, ry, ex, ey);
	canvas.fill_path(gloss);
}

fn draw_graph<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, h: f32, t: f32) {
    let dx = w / 5.0;
    let mut sx = [0.0; 6];
    let mut sy = [0.0; 6];

    let samples = [
        (1.0+(t*1.2345+(t*0.33457).cos()*0.44).sin())*0.5,
        (1.0+(t*0.68363+(t*1.3).cos()*1.55).sin())*0.5,
        (1.0+(t*1.1642+(t*0.33457).cos()*1.24).sin())*0.5,
        (1.0+(t*0.56345+(t*1.63).cos()*0.14).sin())*0.5,
        (1.0+(t*1.6245+(t*0.254).cos()*0.3).sin())*0.5,
        (1.0+(t*0.345+(t*0.03).cos()*0.6).sin())*0.5,
    ];

    for i in 0..6 {
        sx[i] = x+ i as f32 * dx;
		sy[i] = y+h*samples[i]*0.8;
    }

    // Graph background
    let bg = Paint::linear_gradient(x,y,x,y+h, Color::rgba(0,160,192,0), Color::rgba(0,160,192,64));

    canvas.begin_path();
    canvas.move_to(sx[0], sy[0]);

    for i in 1..6 {
        canvas.bezier_to(sx[i-1]+dx*0.5,sy[i-1], sx[i]-dx*0.5,sy[i], sx[i],sy[i]);
    }

    canvas.line_to(x+w, y+h);
    canvas.line_to(x, y+h);
    canvas.fill_path(bg);

    // Graph line
    canvas.begin_path();
    canvas.move_to(sx[0], sy[0] + 2.0);

    for i in 1..6 {
        canvas.bezier_to(sx[i-1]+dx*0.5,sy[i-1], sx[i]-dx*0.5,sy[i], sx[i],sy[i]);
    }

    let mut line = Paint::color(Color::rgba(0,160,192,255));
    line.set_stroke_width(3.0);
    canvas.stroke_path(line);

    // Graph sample pos
    for i in 0..6 {
        let bg = Paint::radial_gradient(sx[i], sy[i] + 2.0, 3.0, 8.0, Color::rgba(0,0,0,32), Color::rgba(0,0,0,0));
        canvas.begin_path();
        canvas.rect(sx[i] - 10.0, sy[i] - 10.0 + 2.0, 20.0, 20.0);
        canvas.fill_path(bg);
    }

    canvas.begin_path();
    for i in 0..6 { canvas.circle(sx[i], sy[i], 4.0); }
    canvas.fill_path(Paint::color(Color::rgba(0,160,192,255)));

    canvas.begin_path();
    for i in 0..6 { canvas.circle(sx[i], sy[i], 2.0); }
    canvas.fill_path(Paint::color(Color::rgba(220,220,220,255)));
}

fn draw_window<T: Renderer>(canvas: &mut Canvas<T>, title: &str, x: f32, y: f32, w: f32, h: f32) {
    let corner_radius = 3.0;

    canvas.save();

    //canvas.global_composite_operation(CompositeOperation::Lighter);

	// Window
	canvas.begin_path();
    canvas.rounded_rect(x, y, w, h, corner_radius);
	canvas.fill_path(Paint::color(Color::rgba(28, 30, 34, 192)));

	// Drop shadow
    let shadow_paint = Paint::box_gradient(x, y + 2.0, w, h, corner_radius * 2.0, 10.0, Color::rgba(0,0,0,128), Color::rgba(0,0,0,0));
	canvas.begin_path();
	canvas.rect(x - 10.0, y - 10.0, w + 20.0, h + 30.0);
	canvas.rounded_rect(x, y, w, h, corner_radius);
	canvas.winding(Winding::CW);
	canvas.fill_path(shadow_paint);

	// Header
	let header_paint = Paint::linear_gradient(x, y, x, y + 15.0, Color::rgba(255,255,255,8), Color::rgba(0,0,0,16));
	canvas.begin_path();
	canvas.rounded_rect(x + 1.0, y + 1.0, w - 2.0, 30.0, corner_radius - 1.0);
	canvas.fill_path(header_paint);

	canvas.begin_path();
	canvas.move_to(x + 0.5, y + 0.5 + 30.0);
	canvas.line_to(x + 0.5 + w - 1.0, y + 0.5 + 30.0);
    canvas.stroke_path(Paint::color(Color::rgba(0, 0, 0, 32)));

    let mut text_paint = Paint::color(Color::rgba(0, 0, 0, 128));
    text_paint.set_font_size(16);
    text_paint.set_font_name("Roboto-Bold");
    text_paint.set_text_align(Align::Center);
    text_paint.set_font_blur(2.0);
	canvas.fill_text(x + (w / 2.0), y + 19.0 + 1.0, title, text_paint);

    text_paint.set_font_blur(0.0);
    text_paint.set_color(Color::rgba(220, 220, 220, 160));

	canvas.fill_text(x + (w / 2.0), y + 19.0, title, text_paint);

    // let bounds = canvas.text_bounds(x + (w / 2.0), y + 19.0, title, text_paint);
    //
    // canvas.begin_path();
    // canvas.rect(bounds[0], bounds[1], bounds[2] - bounds[0], bounds[3] - bounds[1]);
    // canvas.stroke_path(Paint::color(Color::rgba(0, 0, 0, 255)));

	canvas.restore();
}

fn draw_lines<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, _h: f32, t: f32) {
    canvas.save();

    let pad = 5.0;
    let s = w / 9.0 - pad * 2.0;

    let joins = [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel];
    let caps = [LineCap::Butt, LineCap::Round, LineCap::Square];

    let mut pts = [0.0; 4*2];
    pts[0] = -s * 0.25 + (t*0.3).cos() * s * 0.5;
    pts[1] = (t * 0.3).sin() * s * 0.5;
    pts[2] = -s * 0.25;
    pts[3] = 0.0;
    pts[4] = s * 0.25;
    pts[5] = 0.0;
    pts[6] = s * 0.25 + (-t * 0.3).cos() * s * 0.5;
    pts[7] = (-t * 0.3).sin() * s * 0.5;

    for (i, cap) in caps.iter().enumerate() {
        for (j, join) in joins.iter().enumerate() {
            let fx = x + s * 0.5 + (i as f32 * 3.0 + j as f32) / 9.0 * w + pad;
            let fy = y - s * 0.5 + pad;

            let mut paint = Paint::color(Color::rgba(0,0,0,160));
            paint.set_line_cap(*cap);
            paint.set_line_join(*join);
            paint.set_stroke_width(s * 0.3);

            canvas.begin_path();
            canvas.move_to(fx+pts[0], fy+pts[1]);
            canvas.line_to(fx+pts[2], fy+pts[3]);
            canvas.line_to(fx+pts[4], fy+pts[5]);
            canvas.line_to(fx+pts[6], fy+pts[7]);
            canvas.stroke_path(paint);

            paint.set_line_cap(LineCap::Butt);
            paint.set_line_join(LineJoin::Bevel);
            paint.set_stroke_width(1.0);
            paint.set_color(Color::rgba(0,192,255,255));

            canvas.begin_path();
            canvas.move_to(fx+pts[0], fy+pts[1]);
            canvas.line_to(fx+pts[2], fy+pts[3]);
            canvas.line_to(fx+pts[4], fy+pts[5]);
            canvas.line_to(fx+pts[6], fy+pts[7]);
            canvas.stroke_path(paint);
        }
    }

    canvas.restore();
}

fn draw_fills<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32) {
    canvas.save();
    canvas.translate(x, y);

    let mut evenodd_fill = Paint::color(Color::rgb(220, 220, 220));
    evenodd_fill.set_fill_rule(FillRule::EvenOdd);

    canvas.begin_path();
    canvas.move_to(50.0, 0.0);
    canvas.line_to(21.0, 90.0);
    canvas.line_to(98.0, 35.0);
    canvas.line_to(2.0, 35.0);
    canvas.line_to(79.0, 90.0);
    canvas.close_path();
    canvas.fill_path(evenodd_fill);

    canvas.translate(100.0, 0.0);

    let mut nonzero_fill = Paint::color(Color::rgb(220, 220, 220));
    nonzero_fill.set_fill_rule(FillRule::NonZero);

    canvas.begin_path();
    canvas.move_to(50.0, 0.0);
    canvas.line_to(21.0, 90.0);
    canvas.line_to(98.0, 35.0);
    canvas.line_to(2.0, 35.0);
    canvas.line_to(79.0, 90.0);
    canvas.close_path();

    canvas.fill_path(nonzero_fill);

    canvas.restore();
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

/*
fn draw_lines(canvas: &mut Canvas, x: f32, y: f32) {
    canvas.save();
    canvas.translate(x, y);

    let mut paint = Paint::color(Color::hex("#247ba0"));

    let w = 80.0;

    for i in 0..8 {
        paint.set_stroke_width(i as f32);

        canvas.begin_path();
        canvas.move_to(0.0, i as f32 * 10.0);
        canvas.line_to(w, 10.0 + i as f32 * 10.0);
        canvas.stroke_path(paint);
    }

    paint.set_shape_anti_alias(false);

    canvas.translate(95.0, 0.0);

    for i in 0..8 {
        paint.set_stroke_width(i as f32);

        canvas.begin_path();
        canvas.move_to(0.0, i as f32 * 10.0);
        canvas.line_to(w, 10.0 + i as f32 * 10.0);
        canvas.stroke_path(paint);
    }

    canvas.restore();
}

fn draw_state_stack(canvas: &mut Canvas) {
    let rect_width = 150.0;
    let rect_height = 75.0;

    canvas.save();
    // save state 1
    canvas.translate(canvas.width / 2.0, canvas.height / 2.0);

    canvas.save();
    // save state 2
    canvas.rotate(std::f32::consts::PI / 4.0);

    canvas.save();
    // save state 3
    canvas.scale(2.0, 2.0);

    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(Paint::color(Color::hex("#0000FF")));

    canvas.restore();
    // restore state 3
    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(Paint::color(Color::hex("#FF0000")));

    canvas.restore();
    // restore state 2
    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(Paint::color(Color::hex("#FFFF00")));

    canvas.restore();
    // restore state 1
    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(Paint::color(Color::hex("#00FF00")));
}

fn draw_rects(canvas: &mut Canvas, x: f32, y: f32) {

    let fill_paint = Paint::color(Color::hex("#70c1b3"));
    let mut stroke_paint = Paint::color(Color::hex("#247ba0"));
    stroke_paint.set_stroke_width(2.0);

    canvas.save();
    canvas.translate(x, y);

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.fill_path(fill_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.stroke_path(stroke_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rounded_rect(0.0, 0.0, 80.0, 80.0, 10.0);
    canvas.fill_path(fill_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rounded_rect(0.0, 0.0, 80.0, 80.0, 10.0);
    canvas.stroke_path(stroke_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rounded_rect_varying(0.0, 0.0, 80.0, 80.0, 20.0, 20.0, 5.0, 5.0);
    canvas.fill_path(fill_paint);
    canvas.stroke_path(stroke_paint);

    // TODO: Instead of save/restore pairs try doing something with scopes or closures
    // Or use temp var and use drop to restore state
    canvas.translate(95.0, 0.0);

    canvas.save();
    canvas.translate(40.0, 0.0);
    canvas.rotate(45.0f32.to_radians());

    canvas.begin_path();
    canvas.rounded_rect(0.0, 0.0, 55.0, 55.0, 5.0);
    canvas.stroke_path(stroke_paint);
    canvas.restore();

    canvas.translate(95.0, 0.0);
    canvas.save();
    canvas.skew_x(-10.0f32.to_radians());

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.stroke_path(stroke_paint);
    canvas.restore();

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.circle(40.0, 40.0, 40.0);
    canvas.fill_path(fill_paint);
    canvas.stroke_path(stroke_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.ellipse(40.0, 40.0, 30.0, 40.0);
    canvas.fill_path(fill_paint);
    canvas.stroke_path(stroke_paint);

    canvas.translate(95.0, 0.0);
    draw_star(canvas, 0.0, 0.0, 80.0);

    canvas.restore();
}

fn draw_star(canvas: &mut Canvas, cx: f32, cy: f32, scale: f32) {
    canvas.save();

    let paint = Paint::color(Color::hex("#247ba0"));

    let r = 0.45 * scale;
    let tau = 6.2831853;

    canvas.translate(cx + scale*0.5, cy + scale*0.5);

    canvas.begin_path();
    canvas.move_to(r, 0.0);

    for i in 0..7 {
        let theta = 3.0 * i as f32 * tau / 7.0;
        canvas.line_to(theta.cos() * r, theta.sin() * r);
    }

    //canvas.translate(scale * 0.5, scale * 0.5);
    canvas.close();
    //canvas.fill(&path, &paint); // TODO: Why is this not filling ok
    canvas.stroke_path(paint);

    canvas.restore();
}

fn draw_spinner(canvas: &mut Canvas, cx: f32, cy: f32, r: f32, t: f32) {
    let a0 = 0.0 + t * 6.0;
    let a1 = std::f32::consts::PI + t * 6.0;
    let r0 = r;
    let r1 = r * 0.75;

    canvas.save();

    canvas.begin_path();
    canvas.arc(cx, cy, r0, a0, a1, Winding::CW);
    canvas.arc(cx, cy, r1, a1, a0, Winding::CCW);
    canvas.close();

    let ax = cx + a0.cos() * (r0+r1)*0.5;
    let ay = cy + a0.sin() * (r0+r1)*0.5;
    let bx = cx + a1.cos() * (r0+r1)*0.5;
    let by = cy + a1.sin() * (r0+r1)*0.5;

    let paint = Paint::linear_gradient(ax, ay, bx, by, Color::rgba(0, 0, 0, 0), Color::rgba(0, 0, 0, 128));
    canvas.fill_path(paint);

    canvas.restore();
}
*/
