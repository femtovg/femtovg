use std::sync::Arc;

/**
 * Shows how to use `Canvas::filter_image()` to apply a blur filter.
 */
use femtovg::{Canvas, Color, ImageFlags, Paint, Path};
use helpers::WindowSurface;
use instant::Instant;
use resource::resource;
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::Window,
};

mod helpers;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    helpers::start(1000, 600, "Canvas::filter_image example", false);
    #[cfg(target_arch = "wasm32")]
    helpers::start();
}

#[cfg(target_arch = "wasm32")]
use winit::window::Window;

fn run<W: WindowSurface>(mut canvas: Canvas<W::Renderer>, el: EventLoop<()>, mut surface: W, window: Arc<Window>) {
    let image_id = canvas
        .load_image_mem(&resource!("examples/assets/rust-logo.png"), ImageFlags::empty())
        .unwrap();

    let start = Instant::now();

    el.run(move |event, event_loop_window_target| {
        event_loop_window_target.set_control_flow(winit::event_loop::ControlFlow::Poll);

        match event {
            Event::LoopExiting => event_loop_window_target.exit(),
            Event::WindowEvent { ref event, .. } => match event {
                #[cfg(not(target_arch = "wasm32"))]
                WindowEvent::Resized(physical_size) => {
                    surface.resize(physical_size.width, physical_size.height);
                }
                WindowEvent::CloseRequested => event_loop_window_target.exit(),
                WindowEvent::RedrawRequested => {
                    let dpi_factor = window.scale_factor();
                    let window_size = window.inner_size();
                    canvas.set_size(window_size.width, window_size.height, dpi_factor as f32);
                    canvas.clear_rect(0, 0, window_size.width, window_size.height, Color::rgbf(0.2, 0.2, 0.2));

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
                        let bbox = canvas.path_bbox(&path);

                        // Now we need to apply the current canvas transform
                        // to the path bbox:
                        let a = canvas.transform().inverse().transform_point(bbox.minx, bbox.miny);
                        let b = canvas.transform().inverse().transform_point(bbox.maxx, bbox.maxy);

                        canvas.fill_path(
                            &path,
                            &Paint::image(filtered_image.unwrap(), a.0, a.1, b.0 - a.0, b.1 - a.1, 0f32, 1f32),
                        );
                    }

                    canvas.restore();

                    surface.present(&mut canvas);

                    if let Some(img) = filtered_image {
                        canvas.delete_image(img)
                    }
                }
                _ => (),
            },

            Event::AboutToWait => window.request_redraw(),
            _ => (),
        }
    })
    .unwrap();
}
