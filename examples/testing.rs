
use std::time::Instant;

use glutin::event::{Event, WindowEvent, ElementState, KeyboardInput, VirtualKeyCode};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::WindowBuilder;
use glutin::ContextBuilder;
use glutin::{GlRequest, Api};

use rscanvas::{Canvas, Color, Paint, LineCap, LineJoin, Winding, ImageFlags, renderer::{gpu_renderer::GpuRenderer, Void}, math};

fn main() {
    let el = EventLoop::new();
    //let wb = WindowBuilder::new().with_inner_size((800.0, 600.0).into()).with_title("A fantastic window!");
    let wb = WindowBuilder::new().with_title("A fantastic window!");

    let windowed_context = ContextBuilder::new().with_vsync(true).build_windowed(wb, &el).unwrap();
    //let windowed_context = ContextBuilder::new().with_gl(GlRequest::Specific(Api::OpenGl, (1, 0))).with_vsync(true).build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    //let backend = Void::new();
    let backend = GpuRenderer::with_gl(|s| windowed_context.get_proc_address(s) as *const _);
    let mut canvas = Canvas::new(backend);

    canvas.add_font("examples/assets/NotoSans-Regular.ttf");
    // canvas.add_font("/usr/share/fonts/noto/NotoSerif-Regular.ttf");
    // canvas.add_font("/usr/share/fonts/noto/NotoSansArabic-Regular.ttf");
    // canvas.add_font("/usr/share/fonts/TTF/VeraSe.ttf"); // <- Kerning
    //canvas.add_font("/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf");
    //canvas.add_font("/usr/share/fonts/TTF/VeraIt.ttf"); // <- Kerning
    //canvas.add_font("/usr/share/fonts/TTF/TSCu_Times.ttf");

    //canvas.set_font(font_id);

    let image_id = canvas.create_image_file("examples/assets/rust-logo.png", ImageFlags::empty()).expect("Cannot create image");

    //dbg!(canvas.text_bounds(15.0, 300.0, "Hello World"));

    let mut x: f32 = 0.0;
    let mut y: f32 = 0.0;
    let mut rot = 0.0;

    let mut screenshot_image_id = None;

    let start = Instant::now();

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(logical_size) => {
                    let dpi_factor = windowed_context.window().hidpi_factor();
                    windowed_context.resize(logical_size.to_physical(dpi_factor));
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::Left), state: ElementState::Pressed, .. }, .. } => {
                    x -= 1.2;
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::Right), state: ElementState::Pressed, .. }, .. } => {
                    x += 1.2;
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::Up), state: ElementState::Pressed, .. }, .. } => {
                    y -= 1.2;
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::Down), state: ElementState::Pressed, .. }, .. } => {
                    y += 1.2;
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::Q), state: ElementState::Pressed, .. }, .. } => {
                    rot -= 0.5;
                }
                WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(VirtualKeyCode::E), state: ElementState::Pressed, .. }, .. } => {
                    rot += 0.5;
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
                let cpu_start = Instant::now();

                let dpi_factor = windowed_context.window().hidpi_factor();

                let size = windowed_context.window().inner_size().to_physical(dpi_factor);

                let t = start.elapsed().as_secs_f32();

                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgb(255, 255, 255));

                draw_spinner(&mut canvas, 15.0, 285.0, 10.0, t);
                draw_rects(&mut canvas, 15.0, 15.0);
                draw_caps(&mut canvas, 15.0, 110.0);
                draw_joins(&mut canvas, 110.0, 110.0);
                draw_lines(&mut canvas, 205.0, 110.0);
                draw_shadows(&mut canvas);

                //draw_state_stack(&mut canvas);

                if true {

					let combination_marks = format!("Comb. marks: {}{} {}{}", '\u{0061}', '\u{0300}', '\u{0061}', '\u{0328}');
                    let cursive_joining = format!("Cursive Joining: اللغة العربية");
                    let text = format!("Latin text. Ligatures æ fi ﬁ. Kerning VA Wavy. ZWJ? {} {}", combination_marks, cursive_joining);
                    let text = format!("Morbi tincidunt pretium dolor, eu mollis augue tristique quis. Nunc tristique vulputate sem a laoreet. Etiris diam felis, laoreet sit amet nisi eu, pulvinar facilisis massa. ");

                    //let bounds = canvas.text_bounds(15.0, 300.0, text);

                    let mut paint = Paint::color(Color::hex("454545"));

                    let font_size = 20;

                    paint.set_stroke_width(1.0);
                    paint.set_font_size(font_size);
                    //paint.set_letter_spacing(3);
                    //paint.set_font_blur(1.0);
                    paint.set_font_name("BitstreamVeraSerif-Roman".to_string());
                    //paint.set_font_name("NotoSans-Regular".to_string());

					canvas.fill_text(15.0, 220.0, &text, &paint);
                    //canvas.stroke_text(15.0 + x, y + 10.0 + font_size as f32, &line, &paint);
                }

                if let Some(image_id) = screenshot_image_id {
                    let x = size.width as f32 - 512.0;
                    let y = size.height as f32 - 512.0;

                    let paint = Paint::create_image(image_id, x, y, 512.0, 512.0, 0.0, 1.0);

                    canvas.begin_path();
                    canvas.rect(x, y, 512.0, 512.0);
                    canvas.fill_path(&paint);
                    canvas.stroke_path(&Paint::color(Color::hex("454545")));
                }

                // Image
                if true {

                    canvas.save();
                    canvas.translate(10.0, 250.0);

                    let paint = Paint::create_image(image_id, 0.0, 0.0, 293.0, 293.0, 0.0, 1.0);

                    canvas.begin_path();
                    canvas.rect(0.0, 0.0, 293.0, 293.0);
                    canvas.fill_path(&paint);

                    canvas.restore();
                }

                let elapsed = cpu_start.elapsed().as_secs_f32();

                canvas.fill_text(15.0, size.height as f32 - 45.0, &format!("CPU Time: {}", elapsed), &Paint::color(Color::hex("454545")));

                canvas.begin_path();
                canvas.rect(15.0, size.height as f32 - 40.0, 200.0*(elapsed / 0.016), 3.0);
                canvas.fill_path(&Paint::color(Color::hex("000000")));
                canvas.begin_path();
                canvas.rect(15.0, size.height as f32 - 40.0, 200.0, 3.0);
                canvas.stroke_path(&Paint::color(Color::hex("bababa")));

                let gpu_time = Instant::now();

                canvas.end_frame();

                canvas.fill_text(15.0, size.height as f32 - 20.0, &format!("GPU Time: {:?}", gpu_time.elapsed()), &Paint::color(Color::hex("454545")));

                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => {
                windowed_context.window().request_redraw()
            }
            _ => (),
        }
    });
}

fn draw_shadows(canvas: &mut Canvas) {
    canvas.save();

    let paint = Paint::color(Color::hex("#efeeee"));

    let rect_w = 80.0;
    let rect_h = 80.0;
    let x = 395.0;
    let y = 110.0;

    let shadow = Paint::box_gradient(x, y, rect_w, rect_h, 12.0, 16.0, Color::rgba(0, 0, 0, 128), Color::rgba(0, 0, 0, 0));
    canvas.begin_path();
    canvas.rounded_rect(x + 6.0, y + 6.0, rect_w, rect_h, 12.0);
    canvas.fill_path(&shadow);

    let shadow = Paint::box_gradient(x, y, rect_w, rect_h, 12.0, 26.0, Color::rgba(255, 255, 255, 211), Color::rgba(0, 0, 0, 0));
    canvas.begin_path();
    canvas.rounded_rect(x - 6.0, y - 6.0, rect_w, rect_h, 12.0);
    canvas.fill_path(&shadow);

    canvas.begin_path();
    canvas.rounded_rect(x, y, rect_w, rect_h, 12.0);
    canvas.fill_path(&paint);
    canvas.stroke_path(&paint);

    canvas.restore();
}

fn draw_joins(canvas: &mut Canvas, x: f32, y: f32) {
    canvas.save();
    canvas.translate(x, y);

    let w = 50.0;

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.stroke_path(&Paint::color(Color::hex("#247ba0")));

    canvas.scissor(0.0, 0.0, 80.0, 80.0);

    let mut paint = Paint::color(Color::hex("#70c1b3"));
    paint.set_stroke_width(10.0);
    paint.set_line_cap(LineCap::Butt);

    /* TODO: this panics with "attempt to subtract with overflow"
    canvas.set_line_join(LineJoin::Miter);
    canvas.begin_path();
    canvas.move_to(0.0, 40.0);
    canvas.line_to(w/2.0, 10.0);
    canvas.move_to(w, 40.0);
    canvas.stroke();
    */

    canvas.translate(15.0, 0.0);

    paint.set_line_join(LineJoin::Miter);
    canvas.begin_path();
    canvas.move_to(0.0, 40.0);
    canvas.line_to(w/2.0, 10.0);
    canvas.line_to(w, 40.0);
    canvas.stroke_path(&paint);

    canvas.translate(0.0, 25.0);

    paint.set_line_join(LineJoin::Bevel);
    canvas.begin_path();
    canvas.move_to(0.0, 40.0);
    canvas.line_to(w/2.0, 10.0);
    canvas.line_to(w, 40.0);
    canvas.stroke_path(&paint);

    canvas.translate(0.0, 25.0);

    paint.set_line_join(LineJoin::Round);
    canvas.begin_path();
    canvas.move_to(0.0, 40.0);
    canvas.line_to(w/2.0, 10.0);
    canvas.line_to(w, 40.0);
    canvas.stroke_path(&paint);

    canvas.restore();
}

fn draw_caps(canvas: &mut Canvas, x: f32, y: f32) {
    canvas.save();
    canvas.translate(x, y);

    let w = 80.0;

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.stroke_path(&Paint::color(Color::hex("#247ba0")));

    let mut paint = Paint::color(Color::hex("#70c1b3"));

    paint.set_stroke_width(12.0);

    paint.set_line_cap(LineCap::Butt);
    canvas.begin_path();
    canvas.move_to(20.0, 15.0);
    canvas.line_to(60.0, 15.0);
    canvas.stroke_path(&paint);

    paint.set_line_cap(LineCap::Square);
    canvas.begin_path();
    canvas.move_to(20.0, 40.0);
    canvas.line_to(60.0, 40.0);
    canvas.stroke_path(&paint);

    paint.set_line_cap(LineCap::Round);
    canvas.begin_path();
    canvas.move_to(20.0, 65.0);
    canvas.line_to(60.0, 65.0);
    canvas.stroke_path(&paint);

    canvas.restore();
}

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
        canvas.stroke_path(&paint);
    }

    paint.set_shape_anti_alias(false);

    canvas.translate(95.0, 0.0);

    for i in 0..8 {
        paint.set_stroke_width(i as f32);

        canvas.begin_path();
        canvas.move_to(0.0, i as f32 * 10.0);
        canvas.line_to(w, 10.0 + i as f32 * 10.0);
        canvas.stroke_path(&paint);
    }

    canvas.restore();
}

fn draw_state_stack(canvas: &mut Canvas) {
    let rect_width = 150.0;
    let rect_height = 75.0;

    canvas.save();
    // save state 1
    canvas.translate(canvas.size.width / 2.0, canvas.size.height / 2.0);

    canvas.save();
    // save state 2
    canvas.rotate(std::f32::consts::PI / 4.0);

    canvas.save();
    // save state 3
    canvas.scale(2.0, 2.0);

    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(&Paint::color(Color::hex("#0000AA")));

    canvas.restore();
    // restore state 3
    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(&Paint::color(Color::hex("#AA0000")));

    canvas.restore();
    // restore state 2
    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(&Paint::color(Color::hex("#AAAA00")));

    canvas.restore();
    // restore state 1
    canvas.begin_path();
    canvas.rect(rect_width / -2.0, rect_height / -2.0, rect_width, rect_height);
    canvas.fill_path(&Paint::color(Color::hex("#00AA00")));
}

fn draw_rects(canvas: &mut Canvas, x: f32, y: f32) {

    let fill_paint = Paint::color(Color::hex("#70c1b3"));
    let mut stroke_paint = Paint::color(Color::hex("#247ba0"));
    stroke_paint.set_stroke_width(2.0);

    canvas.save();
    canvas.translate(x, y);

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.fill_path(&fill_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.stroke_path(&stroke_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rounded_rect(0.0, 0.0, 80.0, 80.0, 10.0);
    canvas.fill_path(&fill_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rounded_rect(0.0, 0.0, 80.0, 80.0, 10.0);
    canvas.stroke_path(&stroke_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.rounded_rect_varying(0.0, 0.0, 80.0, 80.0, 20.0, 20.0, 5.0, 5.0);
    canvas.fill_path(&fill_paint);
    canvas.stroke_path(&stroke_paint);

    // TODO: Instead of save/restore pairs try doing something with scopes or closures
    // Or use temp var and use drop to restore state
    canvas.translate(95.0, 0.0);

    canvas.save();
    canvas.translate(40.0, 0.0);
    canvas.rotate(45.0f32.to_radians());

    canvas.begin_path();
    canvas.rounded_rect(0.0, 0.0, 55.0, 55.0, 5.0);
    canvas.stroke_path(&stroke_paint);
    canvas.restore();

    canvas.translate(95.0, 0.0);
    canvas.save();
    canvas.skew_x(-10.0f32.to_radians());

    canvas.begin_path();
    canvas.rect(0.0, 0.0, 80.0, 80.0);
    canvas.stroke_path(&stroke_paint);
    canvas.restore();

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.circle(40.0, 40.0, 40.0);
    canvas.fill_path(&fill_paint);
    canvas.stroke_path(&stroke_paint);

    canvas.translate(95.0, 0.0);

    canvas.begin_path();
    canvas.ellipse(40.0, 40.0, 30.0, 40.0);
    canvas.fill_path(&fill_paint);
    canvas.stroke_path(&stroke_paint);

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
    canvas.stroke_path(&paint);

    canvas.restore();
}

fn draw_spinner(canvas: &mut Canvas, cx: f32, cy: f32, r: f32, t: f32) {
    let a0 = 0.0 + t * 6.0;
    let a1 = std::f32::consts::PI + t * 6.0;
    let r0 = r;
    let r1 = r * 0.75;

    canvas.save();

    canvas.begin_path();
    canvas.arc(cx,cy, r0, a0, a1, Winding::CW);
    canvas.arc(cx,cy, r1, a1, a0, Winding::CCW);
    canvas.close();

    let ax = cx + a0.cos() * (r0+r1)*0.5;
    let ay = cy + a0.sin() * (r0+r1)*0.5;
    let bx = cx + a1.cos() * (r0+r1)*0.5;
    let by = cy + a1.sin() * (r0+r1)*0.5;

    let paint = Paint::linear_gradient(ax, ay, bx, by, Color::rgba(0, 0, 0, 0), Color::rgba(0, 0, 0, 128));
    canvas.fill_path(&paint);

    canvas.restore();
}
