/**
 * Shows how to use Canvas::filter_image() to apply a blur filter.
 */
use instant::Instant;

use resource::resource;

use glutin::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    ContextBuilder,
};

use femtovg::{renderer::OpenGl, Canvas, Color, ImageFlags, Paint, Path};

fn main() {
    let window_size = glutin::dpi::PhysicalSize::new(1000, 600);
    let el = EventLoop::new();
    let wb = WindowBuilder::new()
        .with_inner_size(window_size)
        .with_resizable(false)
        .with_title("Canvas::filter_image example");

    let windowed_context = ContextBuilder::new().build_windowed(wb, &el).unwrap();
    let windowed_context = unsafe { windowed_context.make_current().unwrap() };

    let renderer = OpenGl::new(|s| windowed_context.get_proc_address(s) as *const _).expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    canvas.set_size(
        window_size.width as u32,
        window_size.height as u32,
        windowed_context.window().scale_factor() as f32,
    );

    let image_id = canvas
        .load_image_mem(&resource!("examples/assets/rust-logo.png"), ImageFlags::empty())
        .unwrap();

    let start = Instant::now();

    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::LoopDestroyed => return,
            Event::WindowEvent { ref event, .. } => match event {
                WindowEvent::Resized(physical_size) => {
                    windowed_context.resize(*physical_size);
                }
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
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

                let mut filtered_image = None;

                if let Ok(size) = canvas.image_size(image_id) {
                    filtered_image = Some(
                        canvas
                            .create_image_empty(
                                size.0,
                                size.1,
                                femtovg::PixelFormat::Rgba8,
                                femtovg::ImageFlags::PREMULTIPLIED,
                            )
                            .unwrap(),
                    );

                    let now = Instant::now();
                    let t = (now - start).as_secs_f32();
                    let sigma = 2.5 + 2.5 * t.cos();

                    canvas.filter_image(
                        filtered_image.unwrap(),
                        femtovg::ImageFilter::GaussianBlur { sigma },
                        image_id,
                    );

                    let width = size.0 as f32;
                    let height = size.1 as f32;
                    let x = window_size.width as f32 / 2.0;
                    let y = window_size.height as f32 / 2.0;

                    let mut path = Path::new();
                    path.rect(x - width / 2.0, y - height / 2.0, width, height);

                    // Get the bounding box of the path so that we can stretch
                    // the paint to cover it exactly:
                    let bbox = canvas.path_bbox(&mut path);

                    // Now we need to apply the current canvas transform
                    // to the path bbox:
                    let a = canvas.transform().inversed().transform_point(bbox.minx, bbox.miny);
                    let b = canvas.transform().inversed().transform_point(bbox.maxx, bbox.maxy);

                    canvas.fill_path(
                        &mut path,
                        Paint::image(filtered_image.unwrap(), a.0, a.1, b.0 - a.0, b.1 - a.1, 0f32, 1f32),
                    );
                }

                canvas.restore();

                canvas.flush();
                windowed_context.swap_buffers().unwrap();

                filtered_image.map(|img| canvas.delete_image(img));
            }
            Event::MainEventsCleared => windowed_context.window().request_redraw(),
            _ => (),
        }
    });
}
