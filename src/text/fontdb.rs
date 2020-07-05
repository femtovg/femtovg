use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use generational_arena::Arena;

use crate::{ErrorKind, Paint};

use super::{Font, FontId};

pub struct FontDb {
    fonts: Arena<Font>,
}

impl FontDb {
    pub fn new() -> Result<Self, ErrorKind> {
        Ok(Self {
            fonts: Default::default()
        })
    }

    pub fn scan_dir<T: AsRef<Path>>(&mut self, path: T) -> Result<Vec<FontId>, ErrorKind> {
        let path = path.as_ref();
        let mut fonts = Vec::new();

        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    self.scan_dir(&path)?;
                } else {
                    if let Some("ttf") = path.extension().and_then(OsStr::to_str) {
                        fonts.push(self.add_font_file(path)?);
                    }
                }
            }
        }

        Ok(fonts)
    }

    pub fn add_font_file<T: AsRef<Path>>(&mut self, path: T) -> Result<FontId, ErrorKind> {
        let data = std::fs::read(path)?;

        self.add_font_mem(data)
    }

    pub fn add_font_mem(&mut self, data: Vec<u8>) -> Result<FontId, ErrorKind> {
        let font = Font::new(data)?;
        Ok(FontId(self.fonts.insert(font)))
    }

    // pub fn get(&self, id: FontId) -> Option<&Font> {
    //     self.fonts.get(id.0)
    // }

    pub fn get_mut(&mut self, id: FontId) -> Option<&mut Font> {
        self.fonts.get_mut(id.0)
    }

    pub fn find_font<F, T>(&mut self, _text: &str, paint: &Paint, mut callback: F) -> Result<T, ErrorKind>
    where
        F: FnMut((FontId, &mut Font)) -> (bool, T),
    {
        // Try each font in the paint
        for maybe_font_id in paint.font_ids.iter() {
            if let Some(font_id) = maybe_font_id {
                if let Some(font) = self.fonts.get_mut(font_id.0) {
                    let (has_missing, result) = callback((*font_id, font));

                    if !has_missing {
                        return Ok(result);
                    }
                }
            } else {
                break;
            }
        }

        // Try each registered font
        // An optimisation here would be to skip fonts that were tried by the paint
        for (id, font) in &mut self.fonts {
            let (has_missing, result) = callback((FontId(id), font));

            if !has_missing {
                return Ok(result);
            }
        }

        // Just return the first font at this point and let it render .nodef glyphs
        if let Some((id, font)) = self.fonts.iter_mut().next() {
            return Ok(
                callback((FontId(id), font)).1
            );
        }

        Err(ErrorKind::NoFontFound)
    }
}
