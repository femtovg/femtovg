use std::error::Error;
use std::ffi::NulError;
use std::fmt::{self, Display, Formatter};
use std::io;

/// Enum with all possible canvas errors that could occur.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    UnknownError,
    GeneralError(String),
    #[cfg(feature = "image-loading")]
    ImageError(::image::ImageError),
    IoError(io::Error),
    FontParseError,
    NoFontFound,
    FontInfoExtracionError,
    FontSizeTooLargeForAtlas,
    ShaderCompileError(String),
    ShaderLinkError(String),
    RenderTargetError(String),
    ImageIdNotFound,
    ImageUpdateOutOfBounds,
    ImageUpdateWithDifferentFormat,
    UnsuportedImageFromat,
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "canvas error")
    }
}

#[cfg(feature = "image-loading")]
impl From<::image::ImageError> for ErrorKind {
    fn from(error: ::image::ImageError) -> Self {
        Self::ImageError(error)
    }
}

impl From<io::Error> for ErrorKind {
    fn from(error: io::Error) -> Self {
        Self::IoError(error)
    }
}

impl From<NulError> for ErrorKind {
    fn from(error: NulError) -> Self {
        Self::GeneralError(error.to_string())
    }
}

impl Error for ErrorKind {}
