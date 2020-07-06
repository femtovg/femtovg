use generational_arena::Index;

mod renderer;
pub use renderer::{render_atlas, render_direct, TextRendererContext};

mod shaper;
pub use shaper::{ShapedGlyph, Shaper};

mod atlas;
pub use atlas::Atlas;

mod font;
pub use font::{Font, FontMetrics};

mod fontdb;
pub use fontdb::{FontDb};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FontId(pub Index);

#[derive(Clone, Default, Debug)]
pub struct TextMetrics {
    pub x: f32,
    pub y: f32,
    width: f32,
    height: f32,
    pub glyphs: Vec<ShapedGlyph>,
    pub(crate) final_byte_index: usize,
}

impl TextMetrics {
    pub(crate) fn scale(&mut self, scale: f32) {
        self.x *= scale;
        self.y *= scale;
        self.width *= scale;
        self.height *= scale;

        for glyph in &mut self.glyphs {
            glyph.x *= scale;
            glyph.y *= scale;
            glyph.width *= scale;
            glyph.height *= scale;
        }
    }

    /// width of the glyphs as drawn
    pub fn width(&self) -> f32 {
        self.width
    }

    /// height of the glyphs as drawn
    pub fn height(&self) -> f32 {
        self.height
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Baseline {
    /// The text baseline is the top of the em square.
    Top,
    /// The text baseline is the middle of the em square.
    Middle,
    /// The text baseline is the normal alphabetic baseline. Default value.
    Alphabetic,
    // The text baseline is the bottom of the bounding box.
    Bottom,
}

impl Default for Baseline {
    fn default() -> Self {
        Self::Alphabetic
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Align {
    /// The text is left-aligned.
    Left,
    /// The text is centered.
    Center,
    /// The text is right-aligned.
    Right,
}

impl Default for Align {
    fn default() -> Self {
        Self::Left
    }
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum RenderMode {
    Fill,
    Stroke,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::Fill
    }
}