use wgpu::{
    
};

use super::{Command, CommandType, Params, RenderTarget, Renderer};
use crate::{
    // image::ImageFlags,
    renderer::{ImageId, Vertex},
    BlendFactor,
    Color,
    CompositeOperationState,
    ErrorKind,
    FillRule,
    ImageInfo,
    ImageSource,
    ImageStore,
    Rect,
    // Size,
};

pub struct GPUCommandEncoder {
    rps: wgpu::RenderPipeline
}
// pu bn 

pub struct WebGPU {
    device: wgpu::Device,
    debug: bool,
    antialias: bool,
    queue: wgpu::Queue,
    frag_size: usize,
    index_size: usize,

    clear_color: Color
}

impl WebGPU {

}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct WGBlend {
    pub src_rgb: wgpu::BlendFactor,
    pub dst_rgb: wgpu::BlendFactor,
    pub src_alpha: wgpu::BlendFactor,
    pub dst_alpha: wgpu::BlendFactor,
}

// Zero = 0,
// One = 1,
// SrcColor = 2,
// OneMinusSrcColor = 3,
// SrcAlpha = 4,
// OneMinusSrcAlpha = 5,
// DstColor = 6,
// OneMinusDstColor = 7,
// DstAlpha = 8,
// OneMinusDstAlpha = 9,
// SrcAlphaSaturated = 10,
// BlendColor = 11,
// OneMinusBlendColor = 12,

// impl From<BlendFactor> for wgpu::BlendFactor {
//     fn from(a: BlendFactor) -> Self {
//         match a {
//             BlendFactor::Zero => Self::Zero,
//             BlendFactor::One => Self::One,
//             BlendFactor::SrcColor => Self::SrcColor,
//             BlendFactor::OneMinusSrcColor => Self::OneMinusSrcColor,
//             BlendFactor::DstColor => Self::DestinationColor,
//             BlendFactor::OneMinusDstColor => Self::OneMinusDestinationColor,
//             BlendFactor::SrcAlpha => Self::SourceAlpha,
//             BlendFactor::OneMinusSrcAlpha => Self::OneMinusSourceAlpha,
//             BlendFactor::DstAlpha => Self::DestinationAlpha,
//             BlendFactor::OneMinusDstAlpha => Self::OneMinusDestinationAlpha,
//             BlendFactor::SrcAlphaSaturate => Self::SourceAlphaSaturated,
//         }
//     }
// }