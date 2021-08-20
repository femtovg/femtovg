/**
 * Shows how to work with Paint::image() to fill paths.
 * The image is rendered independently of the shape of the path,
 * it does not get stretched to fit the path’s bounding box.
 * If that’s what you want, you have to compute the bounding box with
 * Canvas::path_bbox() and use it to set the cx, cy, width, height values
 * in Paint::image() as shown in this example.
 */
use instant::Instant;

use miniquad::*;

use femtovg::{renderer::Miniquad, Canvas, Color, ImageFlags, ImageId, Paint, Path, PixelFormat, RenderTarget};
struct Stage {
    canvas: Canvas<Miniquad>,

    screen_size: (f32, f32),
    dpi_scale: f32,

    image_id: ImageId,
    start: Instant,
    zoom: i32,
    shape: Shape,
    time_warp: i32,
    swap_directions: bool,
}

enum Shape {
    Rect,
    Ellipse,
    Polar,
}

impl Stage {
    pub fn new(ctx: Context) -> Stage {
        let screen_size = ctx.screen_size();
        let dpi_scale = ctx.dpi_scale();

        let renderer = Miniquad::new(ctx).expect("Cannot create renderer");
        let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

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

        let zoom = 0;
        let shape = Shape::Rect;
        let time_warp = 0;

        eprintln!("Scroll vertically to change zoom, horizontally (or vertically with Shift) to change time warp, click to cycle shape.");

        let swap_directions = false;

        Stage {
            canvas,
            screen_size,
            dpi_scale,
            image_id,
            start,
            zoom,
            shape,
            time_warp,
            swap_directions,
        }
    }
}

impl EventHandlerFree for Stage {
    fn update(&mut self) {}

    fn key_down_event(&mut self, _keycode: KeyCode, keymods: KeyMods, _repeat: bool) {
        self.swap_directions = keymods.shift;
    }

    fn key_up_event(&mut self, _keycode: KeyCode, keymods: KeyMods) {
        self.swap_directions = keymods.shift;
    }

    fn mouse_wheel_event(&mut self, x: f32, y: f32) {
        if self.swap_directions {
            self.time_warp += y as i32;
            self.zoom += x as i32;
        } else {
            self.time_warp += x as i32;
            self.zoom += y as i32;
        }
    }

    fn mouse_button_down_event(&mut self, _button: MouseButton, _x: f32, _y: f32) {
        self.shape = match self.shape {
            Shape::Rect => Shape::Ellipse,
            Shape::Ellipse => Shape::Polar,
            Shape::Polar => Shape::Rect,
        };
    }

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

        let zoom = (self.zoom as f32 / 40.0).exp();
        let time_warp = (self.time_warp as f32 / 20.0).exp();
        self.canvas
            .translate(self.screen_size.0 as f32 / 2.0, self.screen_size.1 as f32 / 2.0);
        self.canvas.scale(zoom, zoom);
        self.canvas
            .translate(self.screen_size.0 as f32 / -2.0, self.screen_size.1 as f32 / -2.0);

        if let Ok(size) = self.canvas.image_size(self.image_id) {
            let now = Instant::now();
            let t = (now - self.start).as_secs_f32() * time_warp;

            // Shake things a bit to notice if we forgot something:
            self.canvas.translate(60.0 * (t / 3.0).cos(), 60.0 * (t / 5.0).sin());

            let rx = 100.0 * t.cos();
            let ry = 100.0 * t.sin();
            let width = f32::max(1.0, size.0 as f32 * zoom + rx);
            let height = f32::max(1.0, size.1 as f32 * zoom + ry);
            let x = self.screen_size.0 as f32 / 2.0;
            let y = self.screen_size.1 as f32 / 2.0;

            let mut path = Path::new();
            match &self.shape {
                Shape::Rect => {
                    path.rect(x - width / 2.0, y - height / 2.0, width, height);
                }
                Shape::Ellipse => {
                    let rx = width / 2.0;
                    let ry = height / 2.0;
                    path.ellipse(x, y, rx, ry);
                }
                Shape::Polar => {
                    const TO_RADIANS: f32 = std::f32::consts::PI / 180.0;
                    for theta in 0..360 {
                        let theta = theta as f32 * TO_RADIANS;
                        let r = width / 3.0 + width / 2.0 * (3.0 * theta + t).cos();
                        let x = x + r * theta.cos();
                        let y = y + r * theta.sin();
                        if path.is_empty() {
                            path.move_to(x, y);
                        } else {
                            path.line_to(x, y);
                        }
                    }
                    path.close();
                    path.circle(x, y, width / 5.0);
                }
            }

            // Get the bounding box of the path so that we can stretch
            // the paint to cover it exactly:
            let bbox = self.canvas.path_bbox(&mut path);

            // Now we need to apply the current canvas transform
            // to the path bbox:
            let a = self.canvas.transform().inversed().transform_point(bbox.minx, bbox.miny);
            let b = self.canvas.transform().inversed().transform_point(bbox.maxx, bbox.maxy);

            self.canvas.fill_path(
                &mut path,
                Paint::image(self.image_id, a.0, a.1, b.0 - a.0, b.1 - a.1, 0f32, 1f32),
            );
        }

        self.canvas.restore();

        self.canvas.flush();

        // ctx.commit_frame();
    }
}

fn main() {
    miniquad::start(
        conf::Conf {
            window_title: "Paint::image example".to_string(),
            window_width: 1000,
            window_height: 600,
            window_resizable: false,
            high_dpi: true,
            ..Default::default()
        },
        |ctx| UserData::free(Stage::new(ctx)),
    );
}
