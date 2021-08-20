/**
 * Shows how to use Canvas::filter_image() to apply a blur filter.
 */
use instant::Instant;

use resource::resource;

use miniquad::*;

use femtovg::{renderer::Miniquad, Canvas, Color, ImageFlags, ImageId, Paint, Path};
struct Stage {
    canvas: Canvas<Miniquad>,

    screen_size: (f32, f32),
    dpi_scale: f32,

    image_id: ImageId,
    start: Instant,
}

impl Stage {
    pub fn new(ctx: Context) -> Stage {
        let screen_size = ctx.screen_size();
        let dpi_scale = ctx.dpi_scale();

        let renderer = Miniquad::new(ctx).expect("Cannot create renderer");
        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

        let image_id = canvas
            .load_image_mem(&resource!("examples/assets/rust-logo.png"), ImageFlags::empty())
            .unwrap();

        let start = Instant::now();

        Stage {
            canvas,
            screen_size,
            dpi_scale,
            image_id,
            start,
        }
    }
}

impl EventHandlerFree for Stage {
    fn update(&mut self) {}

    fn draw(&mut self) {
        self.canvas
            .set_size(self.screen_size.0 as u32, self.screen_size.1 as u32, self.dpi_scale);
        self.canvas.clear_rect(
            0,
            0,
            self.screen_size.0 as u32,
            self.screen_size.1 as u32,
            Color::rgbf(0.2, 0.2, 0.2),
        );

        self.canvas.save();
        self.canvas.reset();

        let mut filtered_image = None;

        if let Ok(size) = self.canvas.image_size(self.image_id) {
            filtered_image = Some(
                self.canvas
                    .create_image_empty(
                        size.0,
                        size.1,
                        femtovg::PixelFormat::Rgba8,
                        femtovg::ImageFlags::PREMULTIPLIED,
                    )
                    .unwrap(),
            );

            let now = Instant::now();
            let t = (now - self.start).as_secs_f32();
            let sigma = 2.5 + 2.5 * t.cos();

            self.canvas.filter_image(
                filtered_image.unwrap(),
                femtovg::ImageFilter::GaussianBlur { sigma },
                self.image_id,
            );

            let width = size.0 as f32;
            let height = size.1 as f32;
            let x = self.screen_size.0 as f32 / 2.0;
            let y = self.screen_size.1 as f32 / 2.0;

            let mut path = Path::new();
            path.rect(x - width / 2.0, y - height / 2.0, width, height);

            // Get the bounding box of the path so that we can stretch
            // the paint to cover it exactly:
            let bbox = self.canvas.path_bbox(&mut path);

            // Now we need to apply the current canvas transform
            // to the path bbox:
            let a = self.canvas.transform().inversed().transform_point(bbox.minx, bbox.miny);
            let b = self.canvas.transform().inversed().transform_point(bbox.maxx, bbox.maxy);

            self.canvas.fill_path(
                &mut path,
                Paint::image(filtered_image.unwrap(), a.0, a.1, b.0 - a.0, b.1 - a.1, 0f32, 1f32),
            );
        }

        self.canvas.restore();

        self.canvas.flush();
        // ctx.commit_frame();

        filtered_image.map(|img| self.canvas.delete_image(img));
    }
}

fn main() {
    miniquad::start(
        conf::Conf {
            window_title: "Canvas::filter_image example".to_string(),
            window_width: 1000,
            window_height: 600,
            window_resizable: false,
            high_dpi: true,
            ..Default::default()
        },
        |ctx| UserData::free(Stage::new(ctx)),
    );
}
