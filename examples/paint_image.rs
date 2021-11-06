/**
 * Shows how to work with Paint::image() to fill paths.
 * The image is rendered independently of the shape of the path,
 * it does not get stretched to fit the path’s bounding box.
 * If that’s what you want, you have to compute the bounding box with
 * Canvas::path_bbox() and use it to set the cx, cy, width, height values
 * in Paint::image() as shown in this example.
 */
use instant::Instant;

use glutin::{
    event::{ElementState, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    ContextBuilder,
};

use femtovg::{renderer::OpenGl, Canvas, Color, ImageFlags, Paint, Path, PixelFormat, RenderTarget};

enum Shape {
    Rect,
    Ellipse,
    Polar,
}

fn main() {
    let window_size = glutin::dpi::PhysicalSize::new(1000, 600);
    let el = EventLoop::new();
    let wb = WindowBuilder::new()
        .with_inner_size(window_size)
        .with_resizable(false)
        .with_title("Paint::image example");

    let windowed_context = ContextBuilder::new().build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new(|s| windowed_context.get_proc_address(s) as *const _).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    canvas.set_size(
        window_size.width as u32,
        window_size.height as u32,
        windowed_context.window().scale_factor() as f32,
    );

    // Prepare the image, in this case a grid.
    let grid_size: usize = 16;
    let image_id = canvas
        .create_image_empty(
            32 * grid_size + 1,
            26 * grid_size + 1,
            PixelFormat::Rgba8,
            ImageFlags::empty(),
        )
        .unwrap();
    canvas.save();
    canvas.reset();
    if let Ok(size) = canvas.image_size(image_id) {
        canvas.set_render_target(RenderTarget::Image(image_id));
        canvas.clear_rect(0, 0, size.0 as u32, size.1 as u32, Color::rgb(0, 0, 0));
        let x_max = (size.0 / grid_size) - 1;
        let y_max = (size.1 / grid_size) - 1;
        for x in 0..(size.0 / grid_size) {
            for y in 0..(size.1 / grid_size) {
                canvas.clear_rect(
                    (x * grid_size + 1) as u32,
                    (y * grid_size + 1) as u32,
                    (grid_size - 1) as u32,
                    (grid_size - 1) as u32,
                    if x == 0 || y == 0 || x == x_max || y == y_max {
                        Color::rgb(40, 80, 40)
                    } else {
                        match (x % 2, y % 2) {
                            (0, 0) => Color::rgb(125, 125, 125),
                            (1, 0) => Color::rgb(155, 155, 155),
                            (0, 1) => Color::rgb(155, 155, 155),
                            (1, 1) => Color::rgb(105, 105, 155),
                            _ => Color::rgb(255, 0, 255),
                        }
                    },
                );
            }
        }
    }
    canvas.restore();

    let start = Instant::now();

    let mut zoom = 0;
    let mut shape = Shape::Rect;
    let mut time_warp = 0;

    eprintln!("Scroll vertically to change zoom, horizontally (or vertically with Shift) to change time warp, click to cycle shape.");

    let mut swap_directions = false;

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::ModifiersChanged(modifiers) => {
                    swap_directions = modifiers.shift();
                }
                WindowEvent::MouseWheel {
                    device_id: _, delta, ..
                } => match delta {
                    glutin::event::MouseScrollDelta::LineDelta(x, y) => {
                        if swap_directions {
                            time_warp += *y as i32;
                            zoom += *x as i32;
                        } else {
                            time_warp += *x as i32;
                            zoom += *y as i32;
                        }
                    }
                    _ => (),
                },
                WindowEvent::MouseInput {
                    device_id: _,
                    state: ElementState::Pressed,
                    ..
                } => {
                    shape = match &shape {
                        Shape::Rect => Shape::Ellipse,
                        Shape::Ellipse => Shape::Polar,
                        Shape::Polar => Shape::Rect,
                    };
                }
                _ => (),
            },
            Event::RedrawRequested(_) => {
                let dpi_factor = windowed_context.window().scale_factor();
                let window_size = windowed_context.window().inner_size();
                canvas.set_size(window_size.width as u32, window_size.height as u32, dpi_factor as f32);
                canvas.clear_rect(
                    0,
                    0,
                    window_size.width as u32,
                    window_size.height as u32,
                    Color::rgbf(0.2, 0.2, 0.2),
                );

                canvas.save();
                canvas.reset();

                let zoom = (zoom as f32 / 40.0).exp();
                let time_warp = (time_warp as f32 / 20.0).exp();
                canvas.translate(window_size.width as f32 / 2.0, window_size.height as f32 / 2.0);
                canvas.scale(zoom, zoom);
                canvas.translate(window_size.width as f32 / -2.0, window_size.height as f32 / -2.0);

                if let Ok(size) = canvas.image_size(image_id) {
                    let now = Instant::now();
                    let t = (now - start).as_secs_f32() * time_warp;

                    // Shake things a bit to notice if we forgot something:
                    canvas.translate(60.0 * (t / 3.0).cos(), 60.0 * (t / 5.0).sin());

                    let rx = 100.0 * t.cos();
                    let ry = 100.0 * t.sin();
                    let width = f32::max(1.0, size.0 as f32 * zoom + rx);
                    let height = f32::max(1.0, size.1 as f32 * zoom + ry);
                    let x = window_size.width as f32 / 2.0;
                    let y = window_size.height as f32 / 2.0;

                    let mut path = Path::new();
                    match &shape {
                        Shape::Rect => {
                            path.rect([x - width / 2.0, y - height / 2.0], width, height);
                        }
                        Shape::Ellipse => {
                            let rx = width / 2.0;
                            let ry = height / 2.0;
                            path.ellipse([x, y], rx, ry);
                        }
                        Shape::Polar => {
                            const TO_RADIANS: f32 = std::f32::consts::PI / 180.0;
                            for theta in 0..360 {
                                let theta = theta as f32 * TO_RADIANS;
                                let r = width / 3.0 + width / 2.0 * (3.0 * theta + t).cos();
                                let x = x + r * theta.cos();
                                let y = y + r * theta.sin();
                                if path.is_empty() {
                                    path.move_to([x, y]);
                                } else {
                                    path.line_to([x, y]);
                                }
                            }
                            path.close();
                            path.circle([x, y], width / 5.0);
                        }
                    }

                    // Get the bounding box of the path so that we can stretch
                    // the paint to cover it exactly:
                    let bbox = canvas.path_bbox(&mut path);

                    // Now we need to apply the current canvas transform
                    // to the path bbox:
                    let a = canvas.transform().inversed().transform_point(bbox.minx, bbox.miny);
                    let b = canvas.transform().inversed().transform_point(bbox.maxx, bbox.maxy);

                    canvas.fill_path(
                        &mut path,
                        Paint::image(image_id, a.0, a.1, b.0 - a.0, b.1 - a.1, 0f32, 1f32),
                    );
                }

                canvas.restore();

                canvas.flush();
                windowed_context.swap_buffers().unwrap();
            }
            Event::MainEventsCleared => windowed_context.window().request_redraw(),
            _ => (),
        }
    });
}
