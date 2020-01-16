
use glutin::event::{Event, WindowEvent, ElementState, KeyboardInput, VirtualKeyCode, MouseButton};
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::WindowBuilder;
use glutin::ContextBuilder;

use rscanvas::{
    Canvas,
    Color,
    Paint,
    LineCap,
    LineJoin,
    Winding,
    ImageFlags,
    renderer::{
        gpu_renderer::GpuRenderer,
        Void
    },
    math
};

//https://www.html5canvastutorials.com/tutorials/html5-canvas-line-caps/

fn lines(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    canvas.save();
    canvas.translate(x + w / 2.0, y + h / 2.0);

    let mut paint = Paint::color(Color::hex("#000000"));

    canvas.save();
    canvas.translate(0.0, -h / 4.0);



    canvas.restore();
    canvas.translate(0.0, h / 4.0);

    canvas.restore();
}

struct Demo {
    name: String,
    draw_fn: fn(&mut Canvas, f32, f32, f32, f32)
}

fn main() {
    let demos = vec![
        Demo { name: String::from("Lines"), draw_fn: line},
    ];

    let el = EventLoop::new();
    let wb = WindowBuilder::new().with_title("rscanvas Demo");
    let windowed_context = ContextBuilder::new().build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let backend = GpuRenderer::with_gl(|s| windowed_context.get_proc_address(s) as *const _);
    let mut canvas = Canvas::new(backend);

    canvas.add_font("examples/assets/NotoSans-Regular.ttf");

    let mut current_demo = 0;
    let mut mousepos = math::Point2D::new(0.0, 0.0);
    let mut mouseup = false;

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(logical_size) => {
                    let dpi_factor = windowed_context.window().hidpi_factor();
                    windowed_context.resize(logical_size.to_physical(dpi_factor));
                }
                WindowEvent::CursorMoved { device_id:_, position: position, modifiers:_} => {
                    mousepos.x = position.x as f32;
                    mousepos.y = position.y as f32;
                }
                WindowEvent::MouseInput { device_id:_, state: ElementState::Pressed, button: MouseButton::Left, modifiers: _} => {
                    mouseup = true;
                }
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit
                }
                _ => (),
            }
            Event::RedrawRequested(_) => {
                let dpi_factor = windowed_context.window().hidpi_factor();
                let size = windowed_context.window().inner_size().to_physical(dpi_factor);

                canvas.set_size(size.width as u32, size.height as u32, dpi_factor as f32);
                canvas.clear_rect(0, 0, size.width as u32, size.height as u32, Color::rgb(255, 255, 255));

                // Sidebar background
                canvas.begin_path();
                canvas.rect(0.0, 0.0, 200.0, size.height as f32);
                canvas.fill_path(&Paint::color(Color::hex("#666666")));

                let mut y = 30.0;

                for (i, demo) in demos.iter().enumerate() {
                    let rect = math::Rect::new(math::Point2D::new(0.0, y - 16.0), math::Size2D::new(200.0, 25.0));

                    if rect.contains(mousepos) {
                        canvas.begin_path();
                        canvas.rect(rect.origin.x, rect.origin.y, rect.size.width, rect.size.height);
                        canvas.fill_path(&Paint::color(Color::hex("#444444")));

                        if mouseup {
                            current_demo = i;
                        }
                    }

                    if i == current_demo {
                        canvas.begin_path();
                        canvas.rect(rect.origin.x, rect.origin.y, rect.size.width, rect.size.height);
                        canvas.fill_path(&Paint::color(Color::hex("#333333")));
                    }

                    let mut text_paint = Paint::color(Color::hex("#ffffff"));
                    text_paint.set_font_name("NotoSans-Regular".to_owned());
                    text_paint.set_font_size(14);

                    canvas.fill_text(20.0, y, &demo.name, &text_paint);

                    y += 25.0
                }

                (demos[current_demo].draw_fn)(&mut canvas, 200.0, 0.0, size.width as f32 - 200.0, size.height as f32);

                canvas.end_frame();

                windowed_context.swap_buffers().unwrap();

                mouseup = false;
            }
            Event::MainEventsCleared => {
                windowed_context.window().request_redraw()
            }
            _ => (),
        }
    });
}
