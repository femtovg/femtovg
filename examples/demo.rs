use std::f32::consts::PI;

use resource::resource;

use instant::Instant;
use winit::event::{ElementState, Event, KeyboardInput, MouseButton, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
//use glutin::{GlRequest, Api};

use femtovg::{
    //CompositeOperation,
    renderer::OpenGl,
    Align,
    Baseline,
    Canvas,
    Color,
    FillRule,
    FontId,
    ImageFlags,
    ImageId,
    LineCap,
    LineJoin,
    Paint,
    Path,
    Renderer,
    Solidity,
};

pub fn quantize(a: f32, d: f32) -> f32 {
    (a / d + 0.5).trunc() * d
}

struct Fonts {
    regular: FontId,
    bold: FontId,
    icons: FontId,
}

fn main() {
    // This provides better error messages in debug mode.
    // It's disabled in release mode so it doesn't bloat up the file size.
    #[cfg(all(debug_assertions, target_arch = "wasm32"))]
    console_error_panic_hook::set_once();

    let el = EventLoop::new();

    #[cfg(not(target_arch = "wasm32"))]
    let (renderer, windowed_context) = {
        use glutin::ContextBuilder;

        let wb = WindowBuilder::new()
            .with_inner_size(winit::dpi::LogicalSize::new(1000, 600))
            .with_title("femtovg demo");

        //let windowed_context = ContextBuilder::new().with_gl(GlRequest::Specific(Api::OpenGlEs, (2, 0))).with_vsync(false).build_windowed(wb, &el).unwrap();
        //let windowed_context = ContextBuilder::new().with_vsync(false).with_multisampling(8).build_windowed(wb, &el).unwrap();
        let windowed_context = ContextBuilder::new().with_vsync(false).build_windowed(wb, &el).unwrap();
        let windowed_context = unsafe { windowed_context.make_current().unwrap() };

        let renderer = OpenGl::new_from_glutin_context(&windowed_context).expect("Cannot create renderer");

        (renderer, windowed_context)
    };

    #[cfg(target_arch = "wasm32")]
    let (renderer, window) = {
        use wasm_bindgen::JsCast;

        let canvas = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .get_element_by_id("canvas")
            .unwrap()
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap();

        use winit::platform::web::WindowBuilderExtWebSys;

        let renderer = OpenGl::new_from_html_canvas(&canvas).expect("Cannot create renderer");

        let window = WindowBuilder::new().with_canvas(Some(canvas)).build(&el).unwrap();

        (renderer, window)
    };

    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    let fonts = Fonts {
        regular: canvas
            .add_font_mem(&resource!("examples/assets/Roboto-Regular.ttf"))
            .expect("Cannot add font"),
        bold: canvas
            .add_font_mem(&resource!("examples/assets/Roboto-Light.ttf"))
            .expect("Cannot add font"),
        icons: canvas
            .add_font_mem(&resource!("examples/assets/entypo.ttf"))
            .expect("Cannot add font"),
    };

    //canvas.add_font("/usr/share/fonts/noto/NotoSansArabic-Regular.ttf").expect("Cannot add font");

    //let image_id = canvas.create_image_file("examples/assets/RoomRender.jpg", ImageFlags::FLIP_Y).expect("Cannot create image");
    //canvas.blur_image(image_id, 10, 1050, 710, 200, 200);

    //let image_id = canvas.load_image_file("examples/assets/RoomRender.jpg", ImageFlags::FLIP_Y).expect("Cannot create image");

    let images = vec![
        canvas
            .load_image_mem(&resource!("examples/assets/images/image1.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image2.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image3.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image4.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image5.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image6.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image7.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image8.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image9.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image10.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image11.jpg"), ImageFlags::empty())
            .unwrap(),
        canvas
            .load_image_mem(&resource!("examples/assets/images/image12.jpg"), ImageFlags::empty())
            .unwrap(),
    ];

    let mut screenshot_image_id = None;

    let start = Instant::now();
    let mut prevt = start;

    let mut mousex = 0.0;
    let mut mousey = 0.0;
    let mut dragging = false;

    let mut perf = PerfGraph::new();

    {
        #[cfg(not(target_arch = "wasm32"))]
        let window = windowed_context.window();
        let dpi_factor = window.scale_factor();
        canvas.set_size(0, 0, dpi_factor as f32);
        canvas.reset();
    }

    el.run(move |event, _, control_flow| {
        #[cfg(not(target_arch = "wasm32"))]
        let window = windowed_context.window();

        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                #[cfg(not(target_arch = "wasm32"))]
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
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
                    winit::event::MouseScrollDelta::LineDelta(_, y) => {
                        let pt = canvas.transform().inversed().transform_point(mousex, mousey);
                        canvas.translate(pt.0, pt.1);
                        canvas.scale(1.0 + (y / 10.0), 1.0 + (y / 10.0));
                        canvas.translate(-pt.0, -pt.1);
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        let y = pos.y as f32;
                        let pt = canvas.transform().inversed().transform_point(mousex, mousey);
                        let rate = 2000.0;
                        canvas.translate(pt.0, pt.1);
                        canvas.scale(1.0 + (y / rate), 1.0 + (y / rate));
                        canvas.translate(-pt.0, -pt.1);
                    }
                },
                WindowEvent::MouseInput {
                    button: MouseButton::Left,
                    state,
                    ..
                } => match state {
                    ElementState::Pressed => dragging = true,
                    ElementState::Released => dragging = false,
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

                let dpi_factor = window.scale_factor();
                let size = window.inner_size();

                let t = start.elapsed().as_secs_f32();

                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgbf(0.3, 0.3, 0.32));

                let winit::dpi::LogicalSize { width, height } = size.to_logical(dpi_factor);

                let pt = canvas.transform().inversed().transform_point(mousex, mousey);
                let rel_mousex = pt.0;
                let rel_mousey = pt.1;

                draw_graph(&mut canvas, 0.0, height / 2.0, width, height / 2.0, t);

                draw_eyes(
                    &mut canvas,
                    width - 250.0,
                    50.0,
                    150.0,
                    100.0,
                    rel_mousex,
                    rel_mousey,
                    t,
                );

                draw_paragraph(
                    &mut canvas,
                    fonts.regular,
                    width - 450.0,
                    50.0,
                    150.0,
                    100.0,
                    rel_mousex,
                    rel_mousey,
                );

                draw_colorwheel(&mut canvas, width - 300.0, height - 350.0, 250.0, 250.0, t);

                draw_lines(&mut canvas, 120.0, height - 50.0, 600.0, 50.0, t);
                draw_widths(&mut canvas, 10.0, 50.0, 30.0);
                draw_fills(&mut canvas, width - 200.0, height - 100.0, mousex, mousey);
                draw_caps(&mut canvas, 10.0, 300.0, 30.0);

                draw_scissor(&mut canvas, 50.0, height - 80.0, t);

                draw_window(&mut canvas, &fonts, "Widgets `n Stuff", 50.0, 50.0, 300.0, 400.0);

                let x = 60.0;
                let mut y = 95.0;

                draw_search_box(&mut canvas, &fonts, "Search", x, y, 280.0, 25.0);
                y += 40.0;
                draw_drop_down(&mut canvas, &fonts, "Effects", 60.0, 135.0, 280.0, 28.0);
                let popy = y + 14.0;
                y += 45.0;

                draw_label(&mut canvas, &fonts, "Login", x, y, 280.0, 20.0);
                y += 25.0;
                draw_edit_box(&mut canvas, &fonts, "Email", x, y, 280.0, 28.0);
                y += 35.0;
                draw_edit_box(&mut canvas, &fonts, "Password", x, y, 280.0, 28.0);
                y += 38.0;
                draw_check_box(&mut canvas, &fonts, "Remember me", x, y, 140.0, 28.0);
                draw_button(
                    &mut canvas,
                    &fonts,
                    Some("\u{E740}"),
                    "Sign in",
                    x + 138.0,
                    y,
                    140.0,
                    28.0,
                    Color::rgba(0, 96, 128, 255),
                );
                y += 45.0;

                // Slider
                draw_label(&mut canvas, &fonts, "Diameter", x, y, 280.0, 20.0);
                y += 25.0;
                draw_edit_box_num(&mut canvas, &fonts, "123.00", "px", x + 180.0, y, 100.0, 28.0);
                draw_slider(&mut canvas, 0.4, x, y, 170.0, 28.0);
                y += 55.0;

                draw_button(
                    &mut canvas,
                    &fonts,
                    Some("\u{E729}"),
                    "Delete",
                    x,
                    y,
                    160.0,
                    28.0,
                    Color::rgba(128, 16, 8, 255),
                );
                draw_button(
                    &mut canvas,
                    &fonts,
                    None,
                    "Cancel",
                    x + 170.0,
                    y,
                    110.0,
                    28.0,
                    Color::rgba(0, 0, 0, 0),
                );

                draw_thumbnails(&mut canvas, 365.0, popy - 30.0, 160.0, 300.0, &images, t);

                /*
                draw_spinner(&mut canvas, 15.0, 285.0, 10.0, t);
                */

                if let Some(image_id) = screenshot_image_id {
                    let x = size.width as f32 - 512.0;
                    let y = size.height as f32 - 512.0;

                    let paint = Paint::image(image_id, x, y, 512.0, 512.0, 0.0, 1.0);

                    let mut path = Path::new();
                    path.rect(x, y, 512.0, 512.0);
                    canvas.fill_path(&mut path, paint);
                    canvas.stroke_path(&mut path, Paint::color(Color::hex("454545")));
                }

                // if true {
                //     let paint = Paint::image(image_id, size.width as f32, 15.0, 1920.0, 1080.0, 0.0, 1.0);
                //     let mut path = Path::new();
                //     path.rect(size.width as f32, 15.0, 1920.0, 1080.0);
                //     canvas.fill_path(&mut path, paint);
                // }

                canvas.save_with(|canvas| {
                    canvas.reset();
                    perf.render(canvas, 5.0, 5.0);
                });

                //canvas.restore();

                canvas.flush();
                #[cfg(not(target_arch = "wasm32"))]
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => {
                //scroll = 1.0;
                window.request_redraw()
            }
            _ => (),
        }
    });
}

fn draw_paragraph<T: Renderer>(
    canvas: &mut Canvas<T>,
    font: FontId,
    x: f32,
    y: f32,
    width: f32,
    _height: f32,
    mx: f32,
    my: f32,
) {
    let text = "This is longer chunk of text.\n\nWould have used lorem ipsum but she was busy jumping over the lazy dog with the fox and all the men who came to the aid of the party.🎉";

    canvas.save();

    let mut paint = Paint::color(Color::rgba(255, 255, 255, 255));
    paint.set_font_size(14.0);
    paint.set_font(&[font]);
    paint.set_text_align(Align::Left);
    paint.set_text_baseline(Baseline::Top);

    let mut gutter_y = 0.0;
    let mut gutter = 0;
    let mut y = y;
    let mut px;
    let mut caret_x;

    let lines = canvas.break_text_vec(width, text, paint).expect("Cannot break text");

    for (line_num, line_range) in lines.into_iter().enumerate() {
        if let Ok(res) = canvas.fill_text(x, y, &text[line_range], paint) {
            let hit = mx > x && mx < (x + width) && my >= y && my < (y + res.height());

            if hit {
                caret_x = if mx < x + res.width() / 2.0 { x } else { x + res.width() };
                px = x;

                for glyph in &res.glyphs {
                    let x0 = glyph.x;
                    let x1 = x0 + glyph.width;
                    let gx = x0 * 0.3 + x1 * 0.7;

                    if mx >= px && mx < gx {
                        caret_x = glyph.x;
                    }

                    px = gx;
                }

                let mut path = Path::new();
                path.rect(caret_x, y, 1.0, res.height());
                canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 192, 0, 255)));

                gutter = line_num + 1;

                gutter_y = y + 14.0 / 2.0;
            }

            y += res.height();
        }
    }

    if gutter > 0 {
        let mut paint = Paint::color(Color::rgba(255, 192, 0, 255));
        paint.set_font_size(12.0);
        paint.set_font(&[font]);
        paint.set_text_align(Align::Right);
        paint.set_text_baseline(Baseline::Middle);

        let text = format!("{}", gutter);

        if let Ok(res) = canvas.measure_text(x - 10.0, gutter_y, &text, paint) {
            let mut path = Path::new();
            path.rounded_rect(
                res.x - 4.0,
                res.y - 2.0,
                res.width() + 8.0,
                res.height() + 4.0,
                (res.height() + 4.0) / 2.0 - 1.0,
            );
            canvas.fill_path(&mut path, paint);

            paint.set_color(Color::rgba(32, 32, 32, 255));
            let _ = canvas.fill_text(x - 10.0, gutter_y, &text, paint);
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

    canvas.restore();
}

fn draw_eyes<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, h: f32, mx: f32, my: f32, t: f32) {
    let ex = w * 0.23;
    let ey = h * 0.5;
    let lx = x + ex;
    let ly = y + ey;
    let rx = x + w - ex;
    let ry = y + ey;
    let br = if ex < ey { ex } else { ey } * 0.5;
    let blink = 1.0 - (t * 0.5).sin().powf(200.0) * 0.8;

    let bg = Paint::linear_gradient(
        x,
        y + h * 0.5,
        x + w * 0.1,
        y + h,
        Color::rgba(0, 0, 0, 32),
        Color::rgba(0, 0, 0, 16),
    );
    let mut path = Path::new();
    path.ellipse(lx + 3.0, ly + 16.0, ex, ey);
    path.ellipse(rx + 3.0, ry + 16.0, ex, ey);
    canvas.fill_path(&mut path, bg);

    let bg = Paint::linear_gradient(
        x,
        y + h * 0.25,
        x + w * 0.1,
        y + h,
        Color::rgba(220, 220, 220, 255),
        Color::rgba(128, 128, 128, 255),
    );
    let mut path = Path::new();
    path.ellipse(lx, ly, ex, ey);
    path.ellipse(rx, ry, ex, ey);
    canvas.fill_path(&mut path, bg);

    let mut dx = (mx - rx) / (ex * 10.0);
    let mut dy = (my - ry) / (ey * 10.0);
    let d = (dx * dx + dy * dy).sqrt();
    if d > 1.0 {
        dx /= d;
        dy /= d;
    }

    dx *= ex * 0.4;
    dy *= ey * 0.5;
    let mut path = Path::new();
    path.ellipse(lx + dx, ly + dy + ey * 0.25 * (1.0 - blink), br, br * blink);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(32, 32, 32, 255)));

    let mut dx = (mx - rx) / (ex * 10.0);
    let mut dy = (my - ry) / (ey * 10.0);
    let d = (dx * dx + dy * dy).sqrt();
    if d > 1.0 {
        dx /= d;
        dy /= d;
    }

    dx *= ex * 0.4;
    dy *= ey * 0.5;
    let mut path = Path::new();
    path.ellipse(rx + dx, ry + dy + ey * 0.25 * (1.0 - blink), br, br * blink);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(32, 32, 32, 255)));

    let gloss = Paint::radial_gradient(
        lx - ex * 0.25,
        ly - ey * 0.5,
        ex * 0.1,
        ex * 0.75,
        Color::rgba(255, 255, 255, 128),
        Color::rgba(255, 255, 255, 0),
    );
    let mut path = Path::new();
    path.ellipse(lx, ly, ex, ey);
    canvas.fill_path(&mut path, gloss);

    let gloss = Paint::radial_gradient(
        rx - ex * 0.25,
        ry - ey * 0.5,
        ex * 0.1,
        ex * 0.75,
        Color::rgba(255, 255, 255, 128),
        Color::rgba(255, 255, 255, 0),
    );
    let mut path = Path::new();
    path.ellipse(rx, ry, ex, ey);
    canvas.fill_path(&mut path, gloss);
}

fn draw_graph<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, h: f32, t: f32) {
    let dx = w / 5.0;
    let mut sx = [0.0; 6];
    let mut sy = [0.0; 6];

    let samples = [
        (1.0 + (t * 1.2345 + (t * 0.33457).cos() * 0.44).sin()) * 0.5,
        (1.0 + (t * 0.68363 + (t * 1.3).cos() * 1.55).sin()) * 0.5,
        (1.0 + (t * 1.1642 + (t * 0.33457).cos() * 1.24).sin()) * 0.5,
        (1.0 + (t * 0.56345 + (t * 1.63).cos() * 0.14).sin()) * 0.5,
        (1.0 + (t * 1.6245 + (t * 0.254).cos() * 0.3).sin()) * 0.5,
        (1.0 + (t * 0.345 + (t * 0.03).cos() * 0.6).sin()) * 0.5,
    ];

    for i in 0..6 {
        sx[i] = x + i as f32 * dx;
        sy[i] = y + h * samples[i] * 0.8;
    }

    // Graph background
    let bg = Paint::linear_gradient(
        x,
        y,
        x,
        y + h,
        Color::rgba(0, 160, 192, 0),
        Color::rgba(0, 160, 192, 64),
    );

    let mut path = Path::new();
    path.move_to(sx[0], sy[0]);

    for i in 1..6 {
        path.bezier_to(sx[i - 1] + dx * 0.5, sy[i - 1], sx[i] - dx * 0.5, sy[i], sx[i], sy[i]);
    }

    path.line_to(x + w, y + h);
    path.line_to(x, y + h);
    canvas.fill_path(&mut path, bg);

    // Graph line
    let mut path = Path::new();
    path.move_to(sx[0], sy[0] + 2.0);

    for i in 1..6 {
        path.bezier_to(sx[i - 1] + dx * 0.5, sy[i - 1], sx[i] - dx * 0.5, sy[i], sx[i], sy[i]);
    }

    let mut line = Paint::color(Color::rgba(0, 160, 192, 255));
    line.set_line_width(3.0);
    canvas.stroke_path(&mut path, line);

    // Graph sample pos
    for i in 0..6 {
        let bg = Paint::radial_gradient(
            sx[i],
            sy[i] + 2.0,
            3.0,
            8.0,
            Color::rgba(0, 0, 0, 32),
            Color::rgba(0, 0, 0, 0),
        );
        let mut path = Path::new();
        path.rect(sx[i] - 10.0, sy[i] - 10.0 + 2.0, 20.0, 20.0);
        canvas.fill_path(&mut path, bg);
    }

    let mut path = Path::new();
    for i in 0..6 {
        path.circle(sx[i], sy[i], 4.0);
    }
    canvas.fill_path(&mut path, Paint::color(Color::rgba(0, 160, 192, 255)));

    let mut path = Path::new();
    for i in 0..6 {
        path.circle(sx[i], sy[i], 2.0);
    }
    canvas.fill_path(&mut path, Paint::color(Color::rgba(220, 220, 220, 255)));
}

fn draw_window<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, title: &str, x: f32, y: f32, w: f32, h: f32) {
    let corner_radius = 3.0;

    canvas.save();

    //canvas.global_composite_operation(CompositeOperation::Lighter);

    // Window
    let mut path = Path::new();
    path.rounded_rect(x, y, w, h, corner_radius);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(28, 30, 34, 192)));

    // Drop shadow
    let shadow_paint = Paint::box_gradient(
        x,
        y + 2.0,
        w,
        h,
        corner_radius * 2.0,
        10.0,
        Color::rgba(0, 0, 0, 128),
        Color::rgba(0, 0, 0, 0),
    );
    let mut path = Path::new();
    path.rect(x - 10.0, y - 10.0, w + 20.0, h + 30.0);
    path.rounded_rect(x, y, w, h, corner_radius);
    path.solidity(Solidity::Hole);
    canvas.fill_path(&mut path, shadow_paint);

    // Header
    let header_paint = Paint::linear_gradient(
        x,
        y,
        x,
        y + 15.0,
        Color::rgba(255, 255, 255, 8),
        Color::rgba(0, 0, 0, 16),
    );
    let mut path = Path::new();
    path.rounded_rect(x + 1.0, y + 1.0, w - 2.0, 30.0, corner_radius - 1.0);
    canvas.fill_path(&mut path, header_paint);

    let mut path = Path::new();
    path.move_to(x + 0.5, y + 0.5 + 30.0);
    path.line_to(x + 0.5 + w - 1.0, y + 0.5 + 30.0);
    canvas.stroke_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 32)));

    let mut text_paint = Paint::color(Color::rgba(0, 0, 0, 32));
    text_paint.set_font_size(16.0);
    text_paint.set_font(&[fonts.bold]);
    text_paint.set_text_align(Align::Center);
    text_paint.set_color(Color::rgba(220, 220, 220, 160));

    let _ = canvas.fill_text(x + (w / 2.0), y + 19.0, title, text_paint);

    // let bounds = canvas.text_bounds(x + (w / 2.0), y + 19.0, title, text_paint);
    //
    // let mut path = Path::new();
    // path.rect(bounds[0], bounds[1], bounds[2] - bounds[0], bounds[3] - bounds[1]);
    // canvas.stroke_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 255)));

    canvas.restore();
}

fn draw_colorwheel<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, h: f32, t: f32) {
    let hue = (t * 0.12).sin();

    canvas.save();

    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    let r1 = if w < h { w } else { h } * 0.5 - 5.0;
    let r0 = r1 - 20.0;
    let aeps = 0.5 / r1;

    for i in 0..6 {
        let a0 = i as f32 / 6.0 * PI * 2.0 - aeps;
        let a1 = (i as f32 + 1.0) / 6.0 * PI * 2.0 + aeps;

        let mut path = Path::new();
        path.arc(cx, cy, r0, a0, a1, Solidity::Hole);
        path.arc(cx, cy, r1, a1, a0, Solidity::Solid);
        path.close();

        let ax = cx + a0.cos() * (r0 + r1) * 0.5;
        let ay = cy + a0.sin() * (r0 + r1) * 0.5;
        let bx = cx + a1.cos() * (r0 + r1) * 0.5;
        let by = cy + a1.sin() * (r0 + r1) * 0.5;

        let paint = Paint::linear_gradient(
            ax,
            ay,
            bx,
            by,
            Color::hsla(a0 / (PI * 2.0), 1.0, 0.55, 1.0),
            Color::hsla(a1 / (PI * 2.0), 1.0, 0.55, 1.0),
        );

        canvas.fill_path(&mut path, paint);
    }

    let mut path = Path::new();
    path.circle(cx, cy, r0 - 0.5);
    path.circle(cx, cy, r1 + 0.5);
    let mut paint = Paint::color(Color::rgba(0, 0, 0, 64));
    paint.set_line_width(1.0);
    canvas.stroke_path(&mut path, paint);

    // Selector
    canvas.save();
    canvas.translate(cx, cy);
    canvas.rotate(hue * PI * 2.0);

    // Marker on
    let mut path = Path::new();
    path.rect(r0 - 1.0, -3.0, r1 - r0 + 2.0, 6.0);
    paint = Paint::color(Color::rgba(255, 255, 255, 192));
    paint.set_line_width(2.0);
    canvas.stroke_path(&mut path, paint);

    paint = Paint::box_gradient(
        r0 - 3.0,
        -5.0,
        r1 - r0 + 6.0,
        10.0,
        2.0,
        4.0,
        Color::rgba(0, 0, 0, 128),
        Color::rgba(0, 0, 0, 0),
    );
    let mut path = Path::new();
    path.rect(r0 - 2.0 - 10.0, -4.0 - 10.0, r1 - r0 + 4.0 + 20.0, 8.0 + 20.0);
    path.rect(r0 - 2.0, -4.0, r1 - r0 + 4.0, 8.0);
    path.solidity(Solidity::Hole);
    canvas.fill_path(&mut path, paint);

    // Center triangle
    let r = r0 - 6.0;
    let ax = (120.0 / 180.0 * PI).cos() * r;
    let ay = (120.0 / 180.0 * PI).sin() * r;
    let bx = (-120.0 / 180.0 * PI).cos() * r;
    let by = (-120.0 / 180.0 * PI).sin() * r;

    let mut path = Path::new();
    path.move_to(r, 0.0);
    path.line_to(ax, ay);
    path.line_to(bx, by);
    path.close();
    paint = Paint::linear_gradient(
        r,
        0.0,
        ax,
        ay,
        Color::hsla(hue, 1.0, 0.5, 1.0),
        Color::rgba(255, 255, 255, 255),
    );
    canvas.fill_path(&mut path, paint);
    paint = Paint::linear_gradient(
        (r + ax) * 0.5,
        ay * 0.5,
        bx,
        by,
        Color::rgba(0, 0, 0, 0),
        Color::rgba(0, 0, 0, 255),
    );
    canvas.fill_path(&mut path, paint);
    paint = Paint::color(Color::rgba(0, 0, 0, 64));
    canvas.stroke_path(&mut path, paint);

    // Select circle on triangle
    let ax = (120.0 / 180.0 * PI).cos() * r * 0.3;
    let ay = (120.0 / 180.0 * PI).sin() * r * 0.4;
    paint = Paint::color(Color::rgba(255, 255, 255, 192));
    paint.set_line_width(2.0);
    let mut path = Path::new();
    path.circle(ax, ay, 5.0);
    canvas.stroke_path(&mut path, paint);

    paint = Paint::radial_gradient(ax, ay, 7.0, 9.0, Color::rgba(0, 0, 0, 64), Color::rgba(0, 0, 0, 0));
    let mut path = Path::new();
    path.rect(ax - 20.0, ay - 20.0, 40.0, 40.0);
    path.circle(ax, ay, 7.0);
    path.solidity(Solidity::Hole);
    canvas.fill_path(&mut path, paint);

    canvas.restore();

    canvas.restore();
}

fn draw_search_box<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, title: &str, x: f32, y: f32, w: f32, h: f32) {
    let corner_radius = (h / 2.0) - 1.0;

    let bg = Paint::box_gradient(
        x,
        y + 1.5,
        w,
        h,
        h / 2.0,
        5.0,
        Color::rgba(0, 0, 0, 16),
        Color::rgba(0, 0, 0, 92),
    );
    let mut path = Path::new();
    path.rounded_rect(x, y, w, h, corner_radius);
    canvas.fill_path(&mut path, bg);

    let mut text_paint = Paint::color(Color::rgba(255, 255, 255, 64));
    text_paint.set_font_size((h * 1.3).round());
    text_paint.set_font(&[fonts.icons]);
    text_paint.set_text_align(Align::Center);
    text_paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x + h * 0.55, y + h * 0.55, "\u{1F50D}", text_paint);

    let mut text_paint = Paint::color(Color::rgba(255, 255, 255, 32));
    text_paint.set_font_size(16.0);
    text_paint.set_font(&[fonts.regular]);
    text_paint.set_text_align(Align::Left);
    text_paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x + h, y + h * 0.5, title, text_paint);

    let mut text_paint = Paint::color(Color::rgba(255, 255, 255, 32));
    text_paint.set_font_size((h * 1.3).round());
    text_paint.set_font(&[fonts.icons]);
    text_paint.set_text_align(Align::Center);
    text_paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x + w - h * 0.55, y + h * 0.45, "\u{2716}", text_paint);
}

fn draw_drop_down<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, title: &str, x: f32, y: f32, w: f32, h: f32) {
    let corner_radius = 4.0;

    let bg = Paint::linear_gradient(x, y, x, y + h, Color::rgba(255, 255, 255, 16), Color::rgba(0, 0, 0, 16));
    let mut path = Path::new();
    path.rounded_rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, corner_radius);
    canvas.fill_path(&mut path, bg);

    let mut path = Path::new();
    path.rounded_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, corner_radius - 0.5);
    canvas.stroke_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 48)));

    let mut text_paint = Paint::color(Color::rgba(255, 255, 255, 160));
    text_paint.set_font_size(16.0);
    text_paint.set_font(&[fonts.regular]);
    text_paint.set_text_align(Align::Left);
    text_paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x + h * 0.3, y + h * 0.5, title, text_paint);

    let mut text_paint = Paint::color(Color::rgba(255, 255, 255, 64));
    text_paint.set_font_size((h * 1.3).round());
    text_paint.set_font(&[fonts.icons]);
    text_paint.set_text_align(Align::Center);
    text_paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x + w - h * 0.5, y + h * 0.45, "\u{E75E}", text_paint);
}

fn draw_label<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, title: &str, x: f32, y: f32, _w: f32, h: f32) {
    let mut text_paint = Paint::color(Color::rgba(255, 255, 255, 128));
    text_paint.set_font_size(14.0);
    text_paint.set_font(&[fonts.regular]);
    text_paint.set_text_align(Align::Left);
    text_paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x, y + h * 0.5, title, text_paint);
}

fn draw_edit_box_base<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, h: f32) {
    let paint = Paint::box_gradient(
        x + 1.0,
        y + 2.5,
        w - 2.0,
        h - 2.0,
        3.0,
        4.0,
        Color::rgba(255, 255, 255, 32),
        Color::rgba(32, 32, 32, 32),
    );

    let mut path = Path::new();
    path.rounded_rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, 3.0);
    canvas.fill_path(&mut path, paint);

    let mut path = Path::new();
    path.rounded_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, 3.5);
    canvas.stroke_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 48)));
}

fn draw_edit_box<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, title: &str, x: f32, y: f32, w: f32, h: f32) {
    draw_edit_box_base(canvas, x, y, w, h);

    let mut text_paint = Paint::color(Color::rgba(255, 255, 255, 64));
    text_paint.set_font_size(16.0);
    text_paint.set_font(&[fonts.regular]);
    text_paint.set_text_align(Align::Left);
    text_paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x + h * 0.5, y + h * 0.5, title, text_paint);
}

fn draw_edit_box_num<T: Renderer>(
    canvas: &mut Canvas<T>,
    fonts: &Fonts,
    title: &str,
    units: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    draw_edit_box_base(canvas, x, y, w, h);

    let mut paint = Paint::color(Color::rgba(255, 255, 255, 64));
    paint.set_font_size(14.0);
    paint.set_font(&[fonts.regular]);
    paint.set_text_align(Align::Right);
    paint.set_text_baseline(Baseline::Middle);

    if let Ok(layout) = canvas.measure_text(0.0, 0.0, units, paint) {
        let _ = canvas.fill_text(x + w - h * 0.3, y + h * 0.5, units, paint);

        paint.set_font_size(16.0);
        paint.set_color(Color::rgba(255, 255, 255, 128));

        let _ = canvas.fill_text(x + w - layout.width() - h * 0.5, y + h * 0.5, title, paint);
    }
}

fn draw_check_box<T: Renderer>(canvas: &mut Canvas<T>, fonts: &Fonts, text: &str, x: f32, y: f32, _w: f32, h: f32) {
    let mut paint = Paint::color(Color::rgba(255, 255, 255, 160));
    paint.set_font_size(14.0);
    paint.set_font(&[fonts.regular]);
    paint.set_text_baseline(Baseline::Middle);

    let _ = canvas.fill_text(x + 28.0, y + h * 0.5, text, paint);

    paint = Paint::box_gradient(
        x + 1.0,
        y + (h * 0.5).floor() - 9.0 + 1.0,
        18.0,
        18.0,
        3.0,
        3.0,
        Color::rgba(0, 0, 0, 32),
        Color::rgba(0, 0, 0, 92),
    );
    let mut path = Path::new();
    path.rounded_rect(x + 1.0, y + (h * 0.5).floor() - 9.0, 18.0, 18.0, 3.0);
    canvas.fill_path(&mut path, paint);

    paint = Paint::color(Color::rgba(255, 255, 255, 128));
    paint.set_font_size(36.0);
    paint.set_font(&[fonts.icons]);
    paint.set_text_align(Align::Center);
    paint.set_text_baseline(Baseline::Middle);
    let _ = canvas.fill_text(x + 9.0 + 2.0, y + h * 0.5, "\u{2713}", paint);
}

fn draw_button<T: Renderer>(
    canvas: &mut Canvas<T>,
    fonts: &Fonts,
    preicon: Option<&str>,
    text: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
) {
    let corner_radius = 4.0;

    let a = if color.is_black() { 16 } else { 32 };

    let bg = Paint::linear_gradient(x, y, x, y + h, Color::rgba(255, 255, 255, a), Color::rgba(0, 0, 0, a));

    let mut path = Path::new();
    path.rounded_rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, corner_radius - 1.0);

    if !color.is_black() {
        canvas.fill_path(&mut path, Paint::color(color));
    }

    canvas.fill_path(&mut path, bg);

    let mut path = Path::new();
    path.rounded_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, corner_radius - 0.5);
    canvas.stroke_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 48)));

    let mut paint = Paint::color(Color::rgba(255, 255, 255, 96));
    paint.set_font_size(15.0);
    paint.set_font(&[fonts.bold]);
    paint.set_text_align(Align::Left);
    paint.set_text_baseline(Baseline::Middle);

    let tw = if let Ok(layout) = canvas.measure_text(0.0, 0.0, text, paint) {
        layout.width()
    } else {
        0.0
    };

    let mut iw = 0.0;

    if let Some(icon) = preicon {
        paint.set_font(&[fonts.icons]);
        paint.set_font_size(h * 1.3);

        if let Ok(layout) = canvas.measure_text(0.0, 0.0, icon, paint) {
            iw = layout.width() + (h * 0.15);
        }

        let _ = canvas.fill_text(x + w * 0.5 - tw * 0.5 - iw * 0.75, y + h * 0.5, icon, paint);
    }

    paint.set_font_size(15.0);
    paint.set_font(&[fonts.regular]);
    paint.set_color(Color::rgba(0, 0, 0, 160));
    let _ = canvas.fill_text(x + w * 0.5 - tw * 0.5 + iw * 0.25, y + h * 0.5 - 1.0, text, paint);
    paint.set_color(Color::rgba(255, 255, 255, 160));
    let _ = canvas.fill_text(x + w * 0.5 - tw * 0.5 + iw * 0.25, y + h * 0.5, text, paint);
}

fn draw_slider<T: Renderer>(canvas: &mut Canvas<T>, pos: f32, x: f32, y: f32, w: f32, h: f32) {
    let cy = y + (h * 0.5).floor();
    let kr = (h * 0.25).floor();

    canvas.save();

    // Slot
    let mut bg = Paint::box_gradient(
        x,
        cy - 2.0 + 1.0,
        w,
        4.0,
        2.0,
        2.0,
        Color::rgba(0, 0, 0, 32),
        Color::rgba(0, 0, 0, 128),
    );
    let mut path = Path::new();
    path.rounded_rect(x, cy - 2.0, w, 4.0, 2.0);
    canvas.fill_path(&mut path, bg);

    // Knob Shadow
    bg = Paint::radial_gradient(
        x + (pos * w).floor(),
        cy + 1.0,
        kr - 3.0,
        kr + 3.0,
        Color::rgba(0, 0, 0, 64),
        Color::rgba(0, 0, 0, 0),
    );
    let mut path = Path::new();
    path.rect(
        x + (pos * w).floor() - kr - 5.0,
        cy - kr - 5.0,
        kr * 2.0 + 5.0 + 5.0,
        kr * 2.0 + 5.0 + 5.0 + 3.0,
    );
    path.circle(x + (pos * w).floor(), cy, kr);
    path.solidity(Solidity::Hole);
    canvas.fill_path(&mut path, bg);

    // Knob
    bg = Paint::linear_gradient(
        x,
        cy - kr,
        x,
        cy + kr,
        Color::rgba(255, 255, 255, 16),
        Color::rgba(0, 0, 0, 16),
    );
    let mut path = Path::new();
    path.circle(x + (pos * w).floor(), cy, kr - 1.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(40, 43, 48, 255)));
    canvas.fill_path(&mut path, bg);

    let mut path = Path::new();
    path.circle(x + (pos * w).floor(), cy, kr - 0.5);
    canvas.stroke_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 92)));

    canvas.restore();
}

fn draw_thumbnails<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, h: f32, images: &[ImageId], t: f32) {
    let corner_radius = 3.0;
    let thumb = 60.0;
    let arry = 30.5;
    let stackh = images.len() as f32 / 2.0 * (thumb + 10.0) + 10.0;
    let u = (1.0 + (t * 0.6).cos()) * 0.5;
    let u2 = (1.0 - (t * 0.2).cos()) * 0.5;

    canvas.save();

    // Drop shadow
    let shadow_paint = Paint::box_gradient(
        x,
        y + 4.0,
        w,
        h,
        corner_radius * 2.0,
        20.0,
        Color::rgba(0, 0, 0, 128),
        Color::rgba(0, 0, 0, 0),
    );
    let mut path = Path::new();
    path.rect(x - 10.0, y - 10.0, w + 20.0, h + 30.0);
    path.rounded_rect(x, y, w, h, corner_radius);
    path.solidity(Solidity::Hole);
    canvas.fill_path(&mut path, shadow_paint);

    // Window
    let mut path = Path::new();
    path.rounded_rect(x, y, w, h, corner_radius);
    path.move_to(x - 10.0, y + arry);
    path.line_to(x + 1.0, y + arry - 11.0);
    path.line_to(x + 1.0, y + arry + 11.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(200, 200, 200, 255)));

    canvas.save();
    canvas.scissor(x, y, w, h);
    canvas.translate(0.0, -(stackh - h) * u);

    let dv = 1.0 / (images.len() as f32 - 1.0);

    for (i, image) in images.iter().enumerate() {
        let mut tx = x + 10.0;
        let mut ty = y + 10.0;
        tx += (i % 2) as f32 * (thumb + 10.0);
        ty += (i / 2) as f32 * (thumb + 10.0);

        let mut iw = thumb;
        let mut ih = thumb;
        let mut ix = 0.0;
        let mut iy = 0.0;

        if let Ok((imgw, imgh)) = canvas.image_size(*image) {
            if imgw < imgh {
                iw = thumb;
                ih = iw * imgh as f32 / imgw as f32;
                ix = 0.0;
                iy = -(ih - thumb) * 0.5;
            } else {
                ih = thumb;
                iw = ih * imgw as f32 / imgh as f32;
                ix = -(iw - thumb) * 0.5;
                iy = 0.0;
            }
        }

        let v = i as f32 * dv;
        let a = ((u2 - v) / dv).max(0.0).min(1.0);

        if a < 1.0 {
            draw_spinner(canvas, tx + thumb / 2.0, ty + thumb / 2.0, thumb * 0.25, t);
        }

        let img_paint = Paint::image(*image, tx + ix, ty + iy, iw, ih, 0.0 / 180.0 * PI, a);
        let mut path = Path::new();
        path.rounded_rect(tx, ty, thumb, thumb, 5.0);
        canvas.fill_path(&mut path, img_paint);

        let shadow_paint = Paint::box_gradient(
            tx - 1.0,
            ty,
            thumb + 2.0,
            thumb + 2.0,
            5.0,
            3.0,
            Color::rgba(0, 0, 0, 128),
            Color::rgba(0, 0, 0, 0),
        );
        let mut path = Path::new();
        path.rect(tx - 5.0, ty - 5.0, thumb + 10.0, thumb + 10.0);
        path.rounded_rect(tx, ty, thumb, thumb, 6.0);
        path.solidity(Solidity::Hole);
        canvas.fill_path(&mut path, shadow_paint);

        let mut path = Path::new();
        path.rounded_rect(tx + 0.5, ty + 0.5, thumb - 1.0, thumb - 1.0, 4.0 - 0.5);
        canvas.stroke_path(&mut path, Paint::color(Color::rgba(255, 255, 255, 192)));
    }

    canvas.restore();

    // Hide fades
    let fade_paint = Paint::linear_gradient(
        x,
        y,
        x,
        y + 6.0,
        Color::rgba(200, 200, 200, 255),
        Color::rgba(200, 200, 200, 0),
    );
    let mut path = Path::new();
    path.rect(x + 4.0, y, w - 8.0, 6.0);
    canvas.fill_path(&mut path, fade_paint);

    let fade_paint = Paint::linear_gradient(
        x,
        y + h,
        x,
        y + h - 6.0,
        Color::rgba(200, 200, 200, 255),
        Color::rgba(200, 200, 200, 0),
    );
    let mut path = Path::new();
    path.rect(x + 4.0, y + h - 6.0, w - 8.0, 6.0);
    canvas.fill_path(&mut path, fade_paint);

    // Scroll bar
    let shadow_paint = Paint::box_gradient(
        x + w - 12.0 + 1.0,
        y + 4.0 + 1.0,
        8.0,
        h - 8.0,
        3.0,
        4.0,
        Color::rgba(0, 0, 0, 32),
        Color::rgba(0, 0, 0, 92),
    );
    let mut path = Path::new();
    path.rounded_rect(x + w - 12.0, y + 4.0, 8.0, h - 8.0, 3.0);
    canvas.fill_path(&mut path, shadow_paint);

    let scrollh = (h / stackh) * (h - 8.0);
    let shadow_paint = Paint::box_gradient(
        x + w - 12.0 - 1.0,
        y + 4.0 + (h - 8.0 - scrollh) * u - 1.0,
        8.0,
        scrollh,
        3.0,
        4.0,
        Color::rgba(220, 220, 220, 255),
        Color::rgba(128, 128, 128, 255),
    );
    let mut path = Path::new();
    path.rounded_rect(
        x + w - 12.0 + 1.0,
        y + 4.0 + 1.0 + (h - 8.0 - scrollh) * u,
        8.0 - 2.0,
        scrollh - 2.0,
        2.0,
    );
    canvas.fill_path(&mut path, shadow_paint);

    canvas.restore();
}

fn draw_lines<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, w: f32, _h: f32, t: f32) {
    canvas.save();

    let pad = 5.0;
    let s = w / 9.0 - pad * 2.0;

    let joins = [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel];
    let caps = [LineCap::Butt, LineCap::Round, LineCap::Square];

    let mut pts = [0.0; 4 * 2];
    pts[0] = -s * 0.25 + (t * 0.3).cos() * s * 0.5;
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

            let mut paint = Paint::color(Color::rgba(0, 0, 0, 160));
            paint.set_line_cap(*cap);
            paint.set_line_join(*join);
            paint.set_line_width(s * 0.3);

            let mut path = Path::new();
            path.move_to(fx + pts[0], fy + pts[1]);
            path.line_to(fx + pts[2], fy + pts[3]);
            path.line_to(fx + pts[4], fy + pts[5]);
            path.line_to(fx + pts[6], fy + pts[7]);
            canvas.stroke_path(&mut path, paint);

            paint.set_line_cap(LineCap::Butt);
            paint.set_line_join(LineJoin::Bevel);
            paint.set_line_width(1.0);
            paint.set_color(Color::rgba(0, 192, 255, 255));

            let mut path = Path::new();
            path.move_to(fx + pts[0], fy + pts[1]);
            path.line_to(fx + pts[2], fy + pts[3]);
            path.line_to(fx + pts[4], fy + pts[5]);
            path.line_to(fx + pts[6], fy + pts[7]);
            canvas.stroke_path(&mut path, paint);
        }
    }

    canvas.restore();
}

fn draw_fills<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, mousex: f32, mousey: f32) {
    canvas.save();
    canvas.translate(x, y);

    let mut evenodd_fill = Paint::color(Color::rgba(220, 220, 220, 120));
    evenodd_fill.set_fill_rule(FillRule::EvenOdd);

    let mut path = Path::new();
    path.move_to(50.0, 0.0);
    path.line_to(21.0, 90.0);
    path.line_to(98.0, 35.0);
    path.line_to(2.0, 35.0);
    path.line_to(79.0, 90.0);
    path.close();

    if canvas.contains_point(&mut path, mousex, mousey, FillRule::EvenOdd) {
        evenodd_fill.set_color(Color::rgb(220, 220, 220));
    }

    canvas.fill_path(&mut path, evenodd_fill);

    canvas.translate(100.0, 0.0);

    let mut nonzero_fill = Paint::color(Color::rgba(220, 220, 220, 120));
    nonzero_fill.set_fill_rule(FillRule::NonZero);

    let mut path = Path::new();
    path.move_to(50.0, 0.0);
    path.line_to(21.0, 90.0);
    path.line_to(98.0, 35.0);
    path.line_to(2.0, 35.0);
    path.line_to(79.0, 90.0);
    path.close();

    if canvas.contains_point(&mut path, mousex, mousey, FillRule::NonZero) {
        nonzero_fill.set_color(Color::rgb(220, 220, 220));
    }

    canvas.fill_path(&mut path, nonzero_fill);

    canvas.restore();
}

fn draw_widths<T: Renderer>(canvas: &mut Canvas<T>, x: f32, mut y: f32, width: f32) {
    canvas.save();

    let mut paint = Paint::color(Color::rgba(0, 0, 0, 255));

    for i in 0..20 {
        let w = (i as f32 + 0.5) * 0.1;
        paint.set_line_width(w);
        let mut path = Path::new();
        path.move_to(x, y);
        path.line_to(x + width, y + width * 0.3);
        canvas.stroke_path(&mut path, paint);
        y += 10.0;
    }

    canvas.restore();
}

fn draw_caps<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, width: f32) {
    let caps = [LineCap::Butt, LineCap::Round, LineCap::Square];
    let line_width = 8.0;

    canvas.save();

    let mut path = Path::new();
    path.rect(x - line_width / 2.0, y, width + line_width, 40.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 255, 255, 32)));

    let mut path = Path::new();
    path.rect(x, y, width, 40.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 255, 255, 32)));

    let mut paint = Paint::color(Color::rgba(0, 0, 0, 255));
    paint.set_line_width(line_width);

    for (i, cap) in caps.iter().enumerate() {
        paint.set_line_cap(*cap);
        let mut path = Path::new();
        path.move_to(x, y + i as f32 * 10.0 + 5.0);
        path.line_to(x + width, y + i as f32 * 10.0 + 5.0);
        canvas.stroke_path(&mut path, paint);
    }

    canvas.restore();
}

fn draw_scissor<T: Renderer>(canvas: &mut Canvas<T>, x: f32, y: f32, t: f32) {
    canvas.save();

    // Draw first rect and set scissor to it's area.
    canvas.translate(x, y);
    canvas.rotate(5.0f32.to_radians());

    let mut path = Path::new();
    path.rect(-20.0, -20.0, 60.0, 40.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 0, 0, 255)));

    canvas.scissor(-20.0, -20.0, 60.0, 40.0);

    // Draw second rectangle with offset and rotation.
    canvas.translate(40.0, 0.0);
    canvas.rotate(t);

    // Draw the intended second rectangle without any scissoring.
    canvas.save();
    canvas.reset_scissor();
    let mut path = Path::new();
    path.rect(-20.0, -10.0, 60.0, 30.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 128, 0, 64)));
    canvas.restore();

    // Draw second rectangle with scissoring.
    //canvas.intersect_scissor(-20.0, -10.0, 60.0, 30.0);
    path.rect(-20.0, -10.0, 60.0, 30.0);
    canvas.fill_path(&mut path, Paint::color(Color::rgba(255, 128, 0, 255)));

    canvas.restore();
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
        canvas.fill_path(&mut path, Paint::color(Color::rgba(0, 0, 0, 128)));

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

fn draw_spinner<T: Renderer>(canvas: &mut Canvas<T>, cx: f32, cy: f32, r: f32, t: f32) {
    let a0 = 0.0 + t * 6.0;
    let a1 = std::f32::consts::PI + t * 6.0;
    let r0 = r;
    let r1 = r * 0.75;

    canvas.save();

    let mut path = Path::new();
    path.arc(cx, cy, r0, a0, a1, Solidity::Hole);
    path.arc(cx, cy, r1, a1, a0, Solidity::Solid);
    path.close();

    let ax = cx + a0.cos() * (r0 + r1) * 0.5;
    let ay = cy + a0.sin() * (r0 + r1) * 0.5;
    let bx = cx + a1.cos() * (r0 + r1) * 0.5;
    let by = cy + a1.sin() * (r0 + r1) * 0.5;

    let paint = Paint::linear_gradient(ax, ay, bx, by, Color::rgba(0, 0, 0, 0), Color::rgba(0, 0, 0, 128));
    canvas.fill_path(&mut path, paint);

    canvas.restore();
}
