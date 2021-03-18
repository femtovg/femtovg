use crate::{
    ErrorKind,
    ImageFlags,
    ImageInfo,
    ImageSource,
    PixelFormat,
};

impl From<PixelFormat> for wgpu::TextureFormat {
    fn from(a: PixelFormat) -> Self {
        match a {
            PixelFormat::Rgba8 => Self::Bgra8Unorm,
            PixelFormat::Rgb8 => unimplemented!("wgpu doesn't support the RGB8 pixel format"),
            PixelFormat::Gray8 => Self::R8Unorm,
        }
    }
}

pub struct WGPUTexture {
    
}
