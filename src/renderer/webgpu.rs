mod wgpu_vec;
pub use wgpu_vec::*;

mod wgpu_queue;
pub use wgpu_queue::*;

mod wgpu_texture;
pub use wgpu_texture::*;

use crate::{
    renderer::{
        ImageId,
        Vertex,
    },
    BlendFactor,
    Color,
    CompositeOperationState,
    ErrorKind,
    FillRule,
    ImageInfo,
    ImageSource,
    ImageStore,
};

use super::{
    Command,
    CommandType,
    Params,
    RenderTarget,
    Renderer,
};

use fnv::FnvHashMap;
use imgref::ImgVec;
use rgb::RGBA8;
use std::borrow::Cow;

pub struct WGPU {}

impl WGPU {
    pub fn new(device: &wgpu::Device) -> Self {
        // let shader = device.create_shader_module(&wgpu::ShaderModuleDescriptor {
        //     label: None,
        //     source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("webgpu/shader.wgsl"))),
        //     flags: wgpu::ShaderFlags::all(),
        // });

        let default_stencil_state = 0;

        let clear_stencil_state = {
            let stencil_state = wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Always,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Keep,
            };

            let state = wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
                clamp_depth: false,
            };
        };

        todo!()
        // Self {

        // }
    }
}

pub struct WGPUTexture {}

impl Renderer for WGPU {
    type Image = WGPUTexture;
    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
        todo!()
    }
    fn render(&mut self, images: &ImageStore<Self::Image>, verts: &[Vertex], commands: &[Command]) {
        todo!()
    }
    fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind> {
        todo!()
    }

    fn update_image(
        &mut self,
        image: &mut Self::Image,
        data: ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        todo!()
    }

    fn delete_image(&mut self, image: Self::Image) {
        todo!()
    }

    fn screenshot(&mut self) -> Result<ImgVec<RGBA8>, ErrorKind> {
        todo!()
    }
}
