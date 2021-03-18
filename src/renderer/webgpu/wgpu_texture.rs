use crate::{
    ErrorKind,
    ImageFlags,
    ImageInfo,
    ImageSource,
    PixelFormat,
};

use super::WGPUContext;

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
    //
    info: ImageInfo,
    tex: wgpu::Texture,
    sampler: wgpu::Sampler,
    context: WGPUContext,
}

impl WGPUTexture {
    pub fn new_pseudo_texture(device: &WGPUContext) -> Self {
        todo!()
    }

    pub fn new(context: &WGPUContext, info: ImageInfo) -> Self {

        assert!(info.format() != PixelFormat::Rgb8);
        let context = context.clone();

        let generate_mipmaps = info.flags().contains(ImageFlags::GENERATE_MIPMAPS);
        let nearest = info.flags().contains(ImageFlags::NEAREST);
        let repeatx = info.flags().contains(ImageFlags::REPEAT_X);
        let repeaty = info.flags().contains(ImageFlags::REPEAT_Y);

        let format = info.format().into();

        let texture_view = context.device()
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("Low Resolution Target"),
            size: wgpu::Extent3d {
                width: 0,
                height: 0,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::RENDER_ATTACHMENT,
        })
        .create_view(&Default::default());
        todo!()
    }
}
