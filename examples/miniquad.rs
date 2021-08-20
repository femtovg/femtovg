use instant::Instant;

use miniquad::*;

use femtovg::{renderer::Miniquad, Canvas, Color, Paint, Path};
struct Stage {
    canvas: Canvas<Miniquad>,

    screen_size: (f32, f32),
    dpi_scale: f32,

    start: Instant,
    size: (f32, f32),
}

impl Stage {
    pub fn new(ctx: Context) -> Stage {
        let screen_size = ctx.screen_size();
        let dpi_scale = ctx.dpi_scale();

        let renderer = Miniquad::new(ctx).expect("Cannot create renderer");
        let canvas = Canvas::new(renderer).expect("Cannot create canvas");

        let start = Instant::now();
        let size = (512., 416.);

        Stage {
            canvas,
            screen_size,
            dpi_scale,
            start,
            size,
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

        self.canvas
            .translate(self.screen_size.0 as f32 / 2.0, self.screen_size.1 as f32 / 2.0);
        self.canvas
            .translate(self.screen_size.0 as f32 / -2.0, self.screen_size.1 as f32 / -2.0);

        let now = Instant::now();
        let t = (now - self.start).as_secs_f32();

        // Shake things a bit to notice if we forgot something:
        self.canvas.translate(60.0 * (t / 3.0).cos(), 60.0 * (t / 5.0).sin());

        let rx = 100.0 * t.cos();
        let ry = 100.0 * t.sin();
        let width = f32::max(1.0, self.size.0 as f32 + rx);
        let height = f32::max(1.0, self.size.1 as f32 + ry);
        let x = self.screen_size.0 as f32 / 2.0;
        let y = self.screen_size.1 as f32 / 2.0;

        let mut path = Path::new();
        path.rect(x - width / 2.0, y - height / 2.0, width, height);

        self.canvas.fill_path(&mut path, Paint::color(Color::rgb(255, 80, 255)));

        self.canvas.restore();

        self.canvas.flush();

        // ctx.commit_frame();
    }
}

fn main() {
    miniquad::start(
        conf::Conf {
            window_title: "pink box example".to_string(),
            window_width: 1000,
            window_height: 600,
            window_resizable: false,
            high_dpi: true,
            ..Default::default()
        },
        |ctx| UserData::free(Stage::new(ctx)),
    );
}
