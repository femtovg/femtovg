
use std::io;
use std::error::Error;
use std::ffi::NulError;
use std::fmt::{self, Display, Formatter};

use image::ImageError;
use ttf_parser as ttf;

use crate::text;

/// Enum with all possible canvas errors that could occur.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    GeneralError(String),
    ImageError(image::ImageError),
    IoError(io::Error),
    FreetypeError(text::freetype::Error),
    TtfParserError(ttf::Error),
    NoFontFound,
    FontInfoExtracionError,
    FontSizeTooLargeForAtlas,
    ShaderCompileError(String),
    ShaderLinkError(String),
    UnsuportedImageFromat(String)
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "canvas error")
    }
}

impl From<ImageError> for ErrorKind {
    fn from(error: ImageError) -> Self {
        Self::ImageError(error)
    }
}

impl From<io::Error> for ErrorKind {
    fn from(error: io::Error) -> Self {
        Self::IoError(error)
    }
}

impl From<text::freetype::Error> for ErrorKind {
    fn from(error: text::freetype::Error) -> Self {
        Self::FreetypeError(error)
    }
}

impl From<ttf::Error> for ErrorKind {
    fn from(error: ttf::Error) -> Self {
        Self::TtfParserError(error)
    }
}

impl From<NulError> for ErrorKind {
    fn from(error: NulError) -> Self {
        Self::GeneralError(error.description().to_string())
    }
}

impl Error for ErrorKind {}
