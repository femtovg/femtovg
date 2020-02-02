
mod text_renderer;
pub use text_renderer::TextRenderer;

mod shaper;
pub use shaper::{
    Shaper,
    ShapedGlyph
};

mod font;
pub use font::Font;

mod freetype;

mod fontdb;
pub use fontdb::{FontDb, FontId};

const GLYPH_PADDING: u32 = 2;

#[derive(Copy, Clone, Default)]
pub struct TextStyle<'a> {
    pub family_name: &'a str,
    pub size: u16,
    pub weight: Weight,
    pub width_class: WidthClass,
    pub font_style: FontStyle,
    pub letter_spacing: f32,
    pub baseline: Baseline,
    pub align: Align,
    pub blur: f32,
    pub render_style: RenderStyle
}

#[derive(Clone, Default, Debug)]
pub struct TextLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub(crate) glyphs: Vec<ShapedGlyph>
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
    Bottom
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
pub enum RenderStyle {
    Fill,
    Stroke {
        width: u16
    }
}

impl Default for RenderStyle {
    fn default() -> Self {
        Self::Fill
    }
}

#[derive(Copy, Clone, Debug, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique
}

impl Default for FontStyle {
    fn default() -> Self {
        Self::Normal
    }
}

//https://docs.microsoft.com/en-us/dotnet/api/system.windows.fontweights?view=netframework-4.8#remarks
#[derive(Copy, Clone, Debug, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum Weight {
    Thin,       // 100
    ExtraLight, // 200
    Light,      // 300
    Normal,     // 400
    Medium,     // 500
    SemiBold,   // 600
    Bold,       // 700
    ExtraBold,  // 800
    Black,      // 900
    ExtraBlack, // 950
    Value(u16)
}

impl Weight {
    pub fn from_value(value: u16) -> Self {
        match value {
            100 => Self::Thin,
            200 => Self::ExtraLight,
            300 => Self::Light,
            400 => Self::Normal,
            500 => Self::Medium,
            600 => Self::SemiBold,
            700 => Self::Bold,
            800 => Self::ExtraBold,
            900 => Self::Black,
            950 => Self::ExtraBlack,
            _ => Weight::Value(value)
        }
    }

    pub fn value(&self) -> u16 {
        match self {
            Self::Thin          => 100,
            Self::ExtraLight    => 200,
            Self::Light         => 300,
            Self::Normal        => 400,
            Self::Medium        => 500,
            Self::SemiBold      => 600,
            Self::Bold          => 700,
            Self::ExtraBold     => 800,
            Self::Black         => 900,
            Self::ExtraBlack    => 950,
            Self::Value(value)  => *value,
        }
    }

    pub fn is_bold(&self) -> bool {
        self.value() > Self::Normal.value()
    }
}

impl Default for Weight {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Copy, Clone, Debug, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum WidthClass {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

impl WidthClass {
    pub fn from_value(value: u16) -> Self {
        match value {
            1 => Self::UltraCondensed,
            2 => Self::ExtraCondensed,
            3 => Self::Condensed,
            4 => Self::SemiCondensed,
            5 => Self::Normal,
            6 => Self::SemiExpanded,
            7 => Self::Expanded,
            8 => Self::ExtraExpanded,
            9 => Self::UltraExpanded,
            _ => Self::Normal
        }
    }

    pub fn value(&self) -> u16 {
        match self {
            Self::UltraCondensed   => 1,
            Self::ExtraCondensed   => 2,
            Self::Condensed        => 3,
            Self::SemiCondensed    => 4,
            Self::Normal           => 5,
            Self::SemiExpanded     => 6,
            Self::Expanded         => 7,
            Self::ExtraExpanded    => 8,
            Self::UltraExpanded    => 9,
        }
    }
}

impl Default for WidthClass {
    fn default() -> Self {
        Self::Normal
    }
}

pub struct Glyph {

}
