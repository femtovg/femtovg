
use std::fs;
use std::io;
use std::fmt;
use std::path::Path;
use std::ffi::OsStr;
use std::error::Error;
use std::convert::TryFrom;
use std::collections::HashMap;

use ttf_parser as ttf;
use font_loader::system_fonts;

use super::{
    Font,
    Weight,
    FontStyle,
    TextStyle,
    WidthClass,
    freetype as ft,
};

type Result<T> = std::result::Result<T, FontDbError>;

#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
pub struct FontId(usize);

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct FontDescription {
    family_name: String,
    weight: Weight,
    font_style: FontStyle,
    width_class: WidthClass,
}

impl FontDescription {
    fn degrade(&mut self) -> bool {
        if !self.family_name.is_empty() {
            self.family_name.clear();
            true
        } else if self.weight != Weight::Normal {
            self.weight = Weight::Normal;
            true
        } else if self.width_class != WidthClass::Normal {
            self.width_class = WidthClass::Normal;
            true
        } else if self.font_style != FontStyle::Normal {
            self.font_style = FontStyle::Normal;
            true
        } else {
            false
        }
    }
}

impl From<&TextStyle<'_>> for FontDescription {
    fn from(style: &TextStyle) -> Self {
        Self {
            family_name: style.family_name.to_owned(),
            weight: style.weight,
            font_style: style.font_style,
            width_class: style.width_class
        }
    }
}

impl From<&FontDescription> for system_fonts::FontProperty {
    fn from(descr: &FontDescription) -> Self {
        let mut builder = system_fonts::FontPropertyBuilder::new();

        if !descr.family_name.is_empty() {
            builder = builder.family(&descr.family_name);
        }

        if descr.weight.is_bold() {
            builder = builder.bold();
        }

        builder = match descr.font_style {
            FontStyle::Italic => builder.italic(),
            FontStyle::Oblique => builder.oblique(),
            _ => builder
        };

        builder.build()
    }
}

impl TryFrom<ttf::Font<'_>> for FontDescription {
    type Error = FontDbError;

    fn try_from(font: ttf::Font<'_>) -> Result<Self> {
        let family_name = font.family_name().ok_or(FontDbError::FontInfoExtrationError)?;
        //let post_script_name = font.post_script_name().ok_or(FontDbError::FontInfoExtrationError)?;
        let weight = Weight::from_value(font.weight().to_number());
        let width_class = WidthClass::from_value(font.width().to_number());

        let font_style = if font.is_oblique() {
            FontStyle::Oblique
        } else if font.is_italic() {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

        //let family_name = format!("{} - {}", post_script_name, family_name);

        Ok(Self {
            family_name,
            weight,
            font_style,
            width_class,
        })
    }
}

pub trait FontDbBackend {

}

pub struct FontDb {
    pub library: ft::Library,
    fonts: Vec<Font>,
    font_descr: HashMap<FontDescription, FontId>,
    fallbacks: HashMap<String, FontId>
}

impl FontDb {

    pub fn new() -> Result<Self> {
        Ok(Self {
            library: ft::Library::init()?,
            fonts: Vec::new(),
            font_descr: HashMap::new(),
            fallbacks: HashMap::new()
        })
    }

    pub fn scan_dir<T: AsRef<Path>>(&mut self, path: T) -> Result<()> {
        let path = path.as_ref();

        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    self.scan_dir(&path)?;
                } else {
                    if let Some("ttf") = path.extension().and_then(OsStr::to_str) {
                        self.add_font_file(path)?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn add_font_file<T: AsRef<Path>>(&mut self, path: T) -> Result<FontId> {
        let data = std::fs::read(path)?;

        self.add_font_mem(data)
    }

    pub fn add_font_mem(&mut self, data: Vec<u8>) -> Result<FontId> {
        let font = ttf::Font::from_data(&data, 0)?;
        let description = FontDescription::try_from(font)?;

        let id;

        if !self.font_descr.contains_key(&description) {
            let face = self.library.new_memory_face(data, 0)?;

            id = FontId(self.fonts.len());
            self.fonts.push(Font::new(id, face));
            self.font_descr.insert(description, id);
        } else {
            id = *self.font_descr.get(&description).unwrap();
        }

        Ok(id)
    }

    pub fn get(&self, id: FontId) -> Option<&Font> {
        self.fonts.get(id.0)
    }

    pub fn get_mut(&mut self, id: FontId) -> Option<&mut Font> {
        self.fonts.get_mut(id.0)
    }

    pub fn fonts_for<'a>(&'a mut self, text: &'a str, style: &'a TextStyle) -> impl Iterator<Item = &'a mut Font> {
        self.fonts.iter_mut()
        // FontsIterator {
        //     text,
        //     style,
        // }
    }

    pub fn find(&mut self, style: &TextStyle) -> Result<&mut Font> {
        let mut description = FontDescription::from(style);

        let id;

        loop {
            if let Some(found_id) = self.font_descr.get(&description) {
                id = *found_id;
                break;
            }

            let property = system_fonts::FontProperty::from(&description);

            if let Some((font_data, _)) = system_fonts::get(&property) {
                id = self.add_font_mem(font_data)?;
                self.font_descr.insert(description, id);
                break;
            }

            if !description.degrade() {
                return Err(FontDbError::NoFontFound);
            }
        }

        self.fonts.get_mut(id.0).ok_or(FontDbError::NoFontFound)
    }

    // TODO: This is slow as hell when there's no font installed on the system that can handle the text
    // Must cache failed attempts as well
    pub fn fallback(&mut self, style: &TextStyle, text: &str) -> Result<&mut Font> {
        // Find a font that has all the codepoints in text
        if let Some(id) = self.fallbacks.get(text) {
            return self.fonts.get_mut(id.0).ok_or(FontDbError::NoFontFound);
        }

        let id;

        let mut description = FontDescription::from(style);

        'outer: loop {
            if let Some(a_id) = self.font_descr.get(&description) {
                if self.fonts[a_id.0].has_chars(text) {
                    id = *a_id;
                    break;
                }
            }

            let mut property = system_fonts::FontProperty::from(&description);

            let sysfonts = system_fonts::query_specific(&mut property);

            for string in &sysfonts {

                let property = system_fonts::FontPropertyBuilder::new().family(string).build();

                if let Some((font_data, _)) = system_fonts::get(&property) {
                    let ttf_font = ttf::Font::from_data(&font_data, 0)?;
                    let has_all = text.chars().all(|c| ttf_font.glyph_index(c).is_ok());

                    if has_all {
                        id = self.add_font_mem(font_data)?;
                        self.font_descr.insert(description, id);
                        self.fallbacks.insert(text.to_string(), id);
                        break 'outer;
                    }
                }
            }

            if !description.degrade() {
                return Err(FontDbError::NoFontFound);
            }
        }

        self.fonts.get_mut(id.0).ok_or(FontDbError::NoFontFound)
    }
}

pub struct FontsIterator<'a> {
    text: &'a str,
    style: &'a TextStyle<'a>
}

impl<'a> FontsIterator<'a> {

}

impl<'a> Iterator for FontsIterator<'a> {
    type Item = &'a mut Font;

    fn next(&mut self) -> Option<&'a mut Font> {
        // if !self.tried_default {
        //
        //
        //     self.tried_default = true;
        // }
        //
        // if self.tried_default {
        //
        // }

        None
    }
}

#[derive(Debug)]
pub enum FontDbError {
    IoError(io::Error),
    FreetypeError(ft::Error),
    TtfParserError(ttf::Error),
    NoFontFound,
    FontInfoExtrationError
}

impl fmt::Display for FontDbError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "font db error")
    }
}

impl From<io::Error> for FontDbError {
    fn from(error: io::Error) -> Self {
        Self::IoError(error)
    }
}

impl From<ft::Error> for FontDbError {
    fn from(error: ft::Error) -> Self {
        Self::FreetypeError(error)
    }
}

impl From<ttf::Error> for FontDbError {
    fn from(error: ttf::Error) -> Self {
        Self::TtfParserError(error)
    }
}

impl Error for FontDbError {}
