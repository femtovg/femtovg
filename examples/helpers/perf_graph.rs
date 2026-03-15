#![allow(unused)]

use femtovg::{Align, Baseline, Canvas, Color, FillRule, Paint, Path, Renderer, TextSettings};

pub struct PerfGraph {
    history_count: usize,
    values: Vec<f32>,
    head: usize,
}

impl PerfGraph {
    pub fn new() -> Self {
        Self {
            history_count: 100,
            values: vec![0.0; 100],
            head: Default::default(),
        }
    }

    pub fn update(&mut self, frame_time: f32) {
        self.head = (self.head + 1) % self.history_count;
        self.values[self.head] = frame_time;
    }

    pub fn get_average(&self) -> f32 {
        self.values.iter().sum::<f32>() / self.history_count as f32
    }

    pub fn render<T: Renderer>(&self, canvas: &mut Canvas<T>, x: f32, y: f32) {
        let avg = self.get_average();

        let w = 200.0;
        let h = 35.0;

        let mut path = Path::new();
        path.rect([x, y], [w, h]);
        canvas.fill_path(&path, &Paint::color(Color::rgba(0, 0, 0, 128)), FillRule::default());

        let mut path = Path::new();
        path.move_to([x, y + h]);

        for i in 0..self.history_count {
            let mut v = 1.0 / (0.00001 + self.values[(self.head + i) % self.history_count]);
            if v > 80.0 {
                v = 80.0;
            }
            let vx = x + (i as f32 / (self.history_count - 1) as f32) * w;
            let vy = y + h - ((v / 80.0) * h);
            path.line_to([vx, vy]);
        }

        path.line_to([x + w, y + h]);
        canvas.fill_path(&path, &Paint::color(Color::rgba(255, 192, 0, 128)), FillRule::default());

        let paint = Paint::color(Color::rgba(240, 240, 240, 255));
        let text = TextSettings::new(&[], 12.0);
        let _ = canvas.fill_text(x + 5.0, y + 13.0, "Frame time", &paint, &text);

        let text = TextSettings::new(&[], 14.0)
            .with_align(Align::Right)
            .with_baseline(Baseline::Top);
        let _ = canvas.fill_text(x + w - 5.0, y, format!("{:.2} FPS", 1.0 / avg), &paint, &text);

        let paint = Paint::color(Color::rgba(240, 240, 240, 200));
        let text = TextSettings::new(&[], 12.0)
            .with_align(Align::Right)
            .with_baseline(Baseline::Alphabetic);
        let _ = canvas.fill_text(
            x + w - 5.0,
            y + h - 5.0,
            format!("{:.2} ms", avg * 1000.0),
            &paint,
            &text,
        );
    }
}
