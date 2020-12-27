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
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct WGBlend {
    pub src_rgb: wgpu::BlendFactor,
    pub dst_rgb: wgpu::BlendFactor,
    pub src_alpha: wgpu::BlendFactor,
    pub dst_alpha: wgpu::BlendFactor,
}


impl From<BlendFactor> for wgpu::BlendFactor {
    fn from(a: BlendFactor) -> Self {
        match a {
            BlendFactor::Zero => Self::Zero,
            BlendFactor::One => Self::One,
            BlendFactor::SrcColor => Self::SrcColor,
            BlendFactor::OneMinusSrcColor => Self::OneMinusSrcColor,
            BlendFactor::DstColor => Self::DstColor,
            BlendFactor::OneMinusDstColor => Self::OneMinusDstColor,
            BlendFactor::SrcAlpha => Self::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha => Self::OneMinusSrcAlpha,
            BlendFactor::DstAlpha => Self::DstAlpha,
            BlendFactor::OneMinusDstAlpha => Self::OneMinusDstAlpha,
            BlendFactor::SrcAlphaSaturate => Self::SrcAlphaSaturated,
        }
    }
}

impl From<CompositeOperationState> for WGBlend {
    fn from(v: CompositeOperationState) -> Self {
        Self {
            src_rgb: v.src_rgb.into(),
            dst_rgb: v.dst_rgb.into(),
            src_alpha: v.src_alpha.into(),
            dst_alpha: v.dst_alpha.into(),
        }
    }
}

pub struct WebGPU {

    device: wgpu::Device, // not present in metalnanovg
    // metal has debug and antialias in the flags, opengl
    // has them as properties
    debug: bool,
    antialias: bool,

    queue: wgpu::Queue,
    // layer: metal::MetalLayer,
    // library: metal::Library,
    // render_encoder: Option<metal::RenderCommandEncoder>,
    frag_size: usize,
    index_size: usize,
    // int flags?
    clear_color: Color,
    // view_size_buffer: GPUVar<Size>,
//a    view_size: Size,
    multiple_buffering: usize,
    // screen_view: [f32; 2],
    // vertex_descriptor: metal::VertexDescriptor,

    // blend_func: Blend,
    // clear_buffer_on_flush: bool,
    //
    // each of fill and stroke have: stencil, anti_alias_stencil and shape_stencil
    //
//a    default_stencil_state: metal::DepthStencilState,
//a
//a    fill_shape_stencil_state: metal::DepthStencilState,
//a    fill_anti_alias_stencil_state_nonzero: wgpu::DepthStencilState,
//a    fill_anti_alias_stencil_state_evenodd: metal::DepthStencilState,
//a    fill_stencil_state_nonzero: metal::DepthStencilState,
//a    fill_stencil_state_evenodd: metal::DepthStencilState,
//a
//a    stroke_shape_stencil_state: metal::DepthStencilState,
//a    stroke_anti_alias_stencil_state: metal::DepthStencilState,
//a    stroke_clear_stencil_state: metal::DepthStencilState,
//a
//a    // clear_rect_stencil_state: metal::DepthStencilState,
//a    rps_cache: RPSCache,
//a    current_rps: Option<Rc<RPS>>,
//a    clear_stencil_rps: metal::RenderPipelineState,
//a    // vert_func: metal::Function,
//a    // frag_func: metal::Function,
//a
//a    // pipeline_pixel_format: metal::MTLPixelFormat,
//a
//a    // pipeline_state: Option<metal::RenderPipelineState>,
//a    // stencil_only_pipeline_state: Option<metal::RenderPipelineState>,
//a
//a    // these are from mvgbuffer
//a    stencil_texture: MtlStencilTexture,
//a    index_buffer: GPUVec<u32>,
//a    vertex_buffer: GPUVec<Vertex>,
//a    // uniform_buffer: GPUVec<Params>,
//a    render_target: RenderTarget,
//a
//a    // todo
//a    pseudo_texture: MtlTexture,
//a    gpu_encoder: GPUCommandEncoder,
//a    // buffers_cache: MtlBuffersCache,
//a
//a    // // we render into this texture and blit with into the target texture
//a    // // as opposed to the target texture directly in order to avoid creating
//a    // // multiple encoders
//a    // pseudo_sampler:
//a
//a    // clear_rect
//a    // clear_rect_vert_func: metal::Function,
//a    // clear_rect_frag_func: metal::Function,
//a    // clear_rect_pipeline_state: Option<metal::RenderPipelineState>,
//a
//a    // Needed for screenshoting,
//a    //
//a    // last_rendered_texture: Option<metal::Texture>,
//a    frame: usize,
}

//a impl From<CGSize> for Size {
//a     fn from(v: CGSize) -> Self {
//a         Self::new(v.width as f32, v.height as f32)
//a     }
//a }

//a pub struct VertexOffsets {
//a     x: usize,
//a     u: usize,
//a }
//a
//a impl VertexOffsets {
//a     pub fn new() -> Self {
//a         // use Vertex;
//a         let x = offset_of!(Vertex, x);
//a         let u = offset_of!(Vertex, u);
//a         Self { x, u }
//a     }
//a }
//a
//a impl Mtl {
//a     pub fn size(&self) -> Size {
//a         // *self.view_size_buffer
//a         self.view_size
//a     }
//a }
//a
//a impl Mtl {
//a     pub fn new(
//a         device: &metal::DeviceRef,
//a         command_queue: &metal::CommandQueueRef,
//a         layer: &metal::MetalLayerRef,
//a     ) -> Self {
//a         let debug = cfg!(debug_assertions);
//a         let antialias = true;
//a
//a         #[cfg(target_os = "macos")]
//a         {
//a             layer.set_opaque(false);
//a         }
//a
//a         let library = {
//a             let root_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
//a             let library_path = root_path.join("src/renderer/mtl/shaders.metallib");
//a             device.new_library_with_file(library_path).expect("library not found")
//a         };
//a
//a         let gpu_encoder = GPUCommandEncoder::new(device, &library);
//a
//a         // let root_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
//a         // let library_path = root_path.join("src/renderer/mtl/shaders.metallib");
//a         // let library = device.new_library_with_file(library_path).expect("library not found");
//a         // let command_queue = device.new_command_queue();
//a         let command_queue = command_queue.to_owned();
//a         let rps_cache = RPSCache::new(device, &library, antialias);
//a
//a         // let vert_func = library
//a         //     .get_function("vertexShader", None)
//a         //     .expect("vert shader not found");
//a
//a         // let frag_func: metal::Function = if antialias {
//a         //     library
//a         //         .get_function("fragmentShaderAA", None)
//a         //         .expect("frag shader not found")
//a         // } else {
//a         //     library
//a         //         .get_function("fragmentShader", None)
//a         //         .expect("frag shader not found")
//a         // };
//a
//a         // let clear_rect_vert_func = library
//a         //     .get_function("clear_rect_vertex", None)
//a         //     .expect("clear_rect_vertex shader not found");
//a
//a         // let clear_rect_frag_func = library
//a         //     .get_function("clear_rect_fragment", None)
//a         //     .expect("clear_rect_fragment shader not found");
//a
//a         // let clear_buffer_on_flush = false;
//a
//a         let drawable_size = layer.drawable_size();
//a         let size: Size = drawable_size.into();
//a
//a         // let vertex_descriptor = {
//a         //     let desc = metal::VertexDescriptor::new();
//a         //     let offsets = VertexOffsets::new();
//a
//a         //     let attrs = desc.attributes().object_at(0).unwrap();
//a         //     attrs.set_format(metal::MTLVertexFormat::Float2);
//a         //     attrs.set_buffer_index(0);
//a         //     attrs.set_offset(offsets.x as u64);
//a
//a         //     let attrs = desc.attributes().object_at(1).unwrap();
//a         //     attrs.set_format(metal::MTLVertexFormat::Float2);
//a         //     attrs.set_buffer_index(0);
//a         //     attrs.set_offset(offsets.u as u64);
//a
//a         //     let layout = desc.layouts().object_at(0).unwrap();
//a         //     layout.set_stride(std::mem::size_of::<Vertex>() as u64);
//a         //     layout.set_step_function(metal::MTLVertexStepFunction::PerVertex);
//a         //     desc
//a         // };
//a
//a         // pseudosampler sescriptor
//a         let pseudo_texture = MtlTexture::new_pseudo_texture(device, &command_queue).unwrap();
//a         #[cfg(debug_assertions)]
//a         pseudo_texture.set_label("pseudo_texture");
//a
//a         let stencil_texture = MtlStencilTexture::new(&device, drawable_size.into());
//a
//a         // Initializes default blend states.
//a         // let blend_func = Blend {
//a         //     src_rgb: metal::MTLBlendFactor::One,
//a         //     dst_rgb: metal::MTLBlendFactor::OneMinusSourceAlpha,
//a         //     src_alpha: metal::MTLBlendFactor::One,
//a         //     dst_alpha: metal::MTLBlendFactor::OneMinusSourceAlpha,
//a         // };
//a
//a         // Initializes stencil states.
//a
//a         // Default stencil state.
//a         let default_stencil_state = {
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("default_stencil_state");
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         let clear_stencil_state = {
//a             let desc = metal::StencilDescriptor::new();
//a             desc.set_stencil_compare_function(metal::MTLCompareFunction::Equal);
//a             desc.set_stencil_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Keep);
//a
//a             desc.set_write_mask(0xff);
//a             desc.set_read_mask(0xff);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("clear_stencil_state");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         // Fill shape stencil.
//a         let fill_shape_stencil_state = {
//a             let front_face_stencil_descriptor = metal::StencilDescriptor::new();
//a             let back_face_stencil_descriptor = metal::StencilDescriptor::new();
//a
//a             front_face_stencil_descriptor.set_stencil_compare_function(metal::MTLCompareFunction::Always);
//a             front_face_stencil_descriptor.set_depth_stencil_pass_operation(metal::MTLStencilOperation::IncrementWrap);
//a             front_face_stencil_descriptor.set_read_mask(0xff);
//a             front_face_stencil_descriptor.set_write_mask(0xff);
//a             back_face_stencil_descriptor.set_stencil_compare_function(metal::MTLCompareFunction::Always);
//a             back_face_stencil_descriptor.set_depth_stencil_pass_operation(metal::MTLStencilOperation::DecrementWrap);
//a             // back_face_stencil_descriptor.set_read_mask(0);
//a             // back_face_stencil_descriptor.set_write_mask(0);
//a             back_face_stencil_descriptor.set_write_mask(0xff);
//a             back_face_stencil_descriptor.set_read_mask(0xff);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a             stencil_descriptor.set_depth_compare_function(metal::MTLCompareFunction::Always);
//a             stencil_descriptor.set_back_face_stencil(Some(&back_face_stencil_descriptor));
//a             stencil_descriptor.set_front_face_stencil(Some(&front_face_stencil_descriptor));
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("fill_shape_stencil_state");
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         // Fill anti-aliased stencil.
//a         let fill_anti_alias_stencil_state_nonzero = {
//a             let desc = metal::StencilDescriptor::new();
//a             desc.set_stencil_compare_function(metal::MTLCompareFunction::Equal);
//a             desc.set_stencil_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Keep);
//a
//a             desc.set_write_mask(0xff);
//a             desc.set_read_mask(0xff);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("fill_anti_alias_stencil_state_nonzero");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         let fill_anti_alias_stencil_state_evenodd = {
//a             let desc = metal::StencilDescriptor::new();
//a             desc.set_stencil_compare_function(metal::MTLCompareFunction::Equal);
//a             desc.set_stencil_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Keep);
//a
//a             desc.set_write_mask(0xff);
//a             desc.set_read_mask(0x1);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("fill_anti_alias_stencil_state_evenodd");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         // Fill stencil.
//a         let fill_stencil_state_nonzero = {
//a             let desc = metal::StencilDescriptor::new();
//a             desc.set_stencil_compare_function(metal::MTLCompareFunction::NotEqual);
//a             desc.set_stencil_failure_operation(metal::MTLStencilOperation::Zero);
//a             desc.set_depth_failure_operation(metal::MTLStencilOperation::Zero);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Zero);
//a
//a             desc.set_read_mask(0xff);
//a             desc.set_write_mask(0xff);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("fill_stencil_state_nonzero");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         let fill_stencil_state_evenodd = {
//a             let desc = metal::StencilDescriptor::new();
//a             desc.set_stencil_compare_function(metal::MTLCompareFunction::NotEqual);
//a             desc.set_stencil_failure_operation(metal::MTLStencilOperation::Zero);
//a             desc.set_depth_failure_operation(metal::MTLStencilOperation::Zero);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Zero);
//a
//a             desc.set_read_mask(0x1);
//a             desc.set_write_mask(0xff);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("fill_stencil_state_evenodd");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         // Stroke shape stencil.
//a         let stroke_shape_stencil_state = {
//a             let desc = metal::StencilDescriptor::new();
//a             desc.set_stencil_compare_function(metal::MTLCompareFunction::Equal);
//a             desc.set_stencil_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_failure_operation(metal::MTLStencilOperation::Keep);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::IncrementClamp);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("stroke_shape_stencil_state");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         // Stroke anti-aliased stencil.
//a         let stroke_anti_alias_stencil_state = {
//a             let desc = metal::StencilDescriptor::new();
//a             // desc.set_stencil_compare_function(metal::MTLCompareFunction::Equal);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Keep);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("stroke_anti_alias_stencil_state");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         // Stroke clear stencil.
//a         let stroke_clear_stencil_state = {
//a             let desc = metal::StencilDescriptor::new();
//a             desc.set_stencil_compare_function(metal::MTLCompareFunction::Always);
//a             desc.set_stencil_failure_operation(metal::MTLStencilOperation::Zero);
//a             desc.set_depth_failure_operation(metal::MTLStencilOperation::Zero);
//a             desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Zero);
//a
//a             let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a             // stencil_descriptor.set_depth_write_enabled(true);
//a             #[cfg(debug_assertions)]
//a             stencil_descriptor.set_label("stroke_clear_stencil_state");
//a             stencil_descriptor.set_back_face_stencil(None);
//a             stencil_descriptor.set_front_face_stencil(Some(&desc));
//a             device.new_depth_stencil_state(&stencil_descriptor)
//a         };
//a
//a         // let clear_rect_stencil_state = {
//a         //     let desc = metal::StencilDescriptor::new();
//a         //     desc.set_stencil_compare_function(metal::MTLCompareFunction::NotEqual);
//a         //     desc.set_stencil_failure_operation(metal::MTLStencilOperation::Zero);
//a         //     desc.set_depth_failure_operation(metal::MTLStencilOperation::Zero);
//a         //     desc.set_depth_stencil_pass_operation(metal::MTLStencilOperation::Zero);
//a
//a         //     let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a         //     // stencil_descriptor.set_depth_write_enabled(true);
//a         //     #[cfg(debug_assertions)]
//a         //     stencil_descriptor.set_label("clear_rect_stencil_state");
//a         //     stencil_descriptor.set_back_face_stencil(None);
//a         //     stencil_descriptor.set_front_face_stencil(Some(&desc));
//a         //     device.new_depth_stencil_state(&stencil_descriptor)
//a         //     // let front_face_stencil_descriptor = metal::StencilDescriptor::new();
//a         //     // let back_face_stencil_descriptor = metal::StencilDescriptor::new();
//a
//a         //     // front_face_stencil_descriptor.set_stencil_compare_function(metal::MTLCompareFunction::NotEqual);
//a         //     // front_face_stencil_descriptor.set_depth_stencil_pass_operation(metal::MTLStencilOperation::IncrementWrap);
//a         //     // // front_face_stencil_descriptor.set_read_mask(0);
//a         //     // // front_face_stencil_descriptor.set_write_mask(0);
//a         //     // back_face_stencil_descriptor.set_stencil_compare_function(metal::MTLCompareFunction::NotEqual);
//a         //     // back_face_stencil_descriptor.set_depth_stencil_pass_operation(metal::MTLStencilOperation::DecrementWrap);
//a         //     // // back_face_stencil_descriptor.set_read_mask(0);
//a         //     // // back_face_stencil_descriptor.set_write_mask(0);
//a
//a         //     // let stencil_descriptor = metal::DepthStencilDescriptor::new();
//a         //     // stencil_descriptor.set_depth_compare_function(metal::MTLCompareFunction::Always);
//a         //     // stencil_descriptor.set_back_face_stencil(Some(&back_face_stencil_descriptor));
//a         //     // stencil_descriptor.set_front_face_stencil(Some(&front_face_stencil_descriptor));
//a         //     // // stencil_descriptor.set_depth_write_enabled(true);
//a         //     // #[cfg(debug_assertions)]
//a         //     // stencil_descriptor.set_label("clear_rect_stencil_state");
//a         //     // device.new_depth_stencil_state(&stencil_descriptor)
//a         // };
//a
//a         let clear_stencil_rps = clear_stencil_pipeline_state(device, &library);
//a         Self {
//a             gpu_encoder,
//a             clear_stencil_rps,
//a             multiple_buffering: 3,
//a             layer: layer.to_owned(),
//a             // buffers_cache: MtlBuffersCache::new(&device, 3),
//a             debug,
//a             antialias,
//a             rps_cache,
//a             current_rps: None,
//a             // blend_func,
//a             // render_encoder: None,
//a             // todo check what is this initialized to
//a             // view_size_buffer: GPUVar::with_value(&device, size),
//a             view_size: size,
//a             command_queue,
//a             // frag_func,
//a             // vert_func,
//a             // pipeline_state: None,
//a             // screen_view: [0.0, 0.0],
//a             // clear_buffer_on_flush,
//a             default_stencil_state,
//a             fill_shape_stencil_state,
//a             fill_anti_alias_stencil_state_nonzero,
//a             fill_anti_alias_stencil_state_evenodd,
//a             fill_stencil_state_nonzero,
//a             fill_stencil_state_evenodd,
//a             stroke_shape_stencil_state,
//a             stroke_anti_alias_stencil_state,
//a             stroke_clear_stencil_state,
//a             // clear_rect_stencil_state,
//a             frag_size: std::mem::size_of::<Params>(),
//a             index_size: 4, // MTLIndexTypeUInt32
//a             // stencil_only_pipeline_state: None,
//a             stencil_texture,
//a             index_buffer: GPUVec::with_capacity(&device, 32),
//a             vertex_buffer: GPUVec::with_capacity(&device, 32),
//a             // uniform_buffer: GPUVec::with_capacity(&device, 2),
//a             // vertex_descriptor: vertex_descriptor.to_owned(),
//a             // pipeline_pixel_format: metal::MTLPixelFormat::Invalid,
//a             render_target: RenderTarget::Screen,
//a             pseudo_texture,
//a             clear_color: Color::blue(),
//a             device: device.to_owned(),
//a
//a             // clear_rect_vert_func,
//a             // clear_rect_frag_func,
//a             // clear_rect_pipeline_state: None,
//a             // last_rendered_texture: None,
//a             frame: 0,
//a         }
//a     }
//a
//a     /// updaterenderpipelinstateforblend
//a     pub fn set_composite_operation(
//a         &mut self,
//a         blend_func: CompositeOperationState,
//a         pixel_format: metal::MTLPixelFormat,
//a     ) {
//a         // println!("set_composite operation {:?}", pixel_format);
//a         let blend_func: Blend = blend_func.into();
//a         if let Some(current_rps) = self.current_rps.as_ref() {
//a             if current_rps.blend_func == blend_func && current_rps.pixel_format == pixel_format {
//a                 return;
//a             }
//a         }
//a
//a         self.current_rps = Some(self.rps_cache.get(blend_func, pixel_format));
//a
//a         // println!("rps_cache.len() == {}", self.rps_cache.len());
//a         // if self.pipeline_state.is_some()
//a         //     && self.stencil_only_pipeline_state.is_some()
//a         //     && self.pipeline_pixel_format == pixel_format
//a         //     && self.blend_func == blend_func
//a         // {
//a         //     // println!("skipping setting composite op");
//a         //     return;
//a         // }
//a         // println!("setting composite op for {:?} and {:?}", blend_func, pixel_format);
//a
//a         // let desc = metal::RenderPipelineDescriptor::new();
//a         // let color_attachment_desc = desc.color_attachments().object_at(0).unwrap();
//a         // color_attachment_desc.set_pixel_format(pixel_format);
//a
//a         // // println!("blend: {:?}", blend_func);
//a         // desc.set_stencil_attachment_pixel_format(metal::MTLPixelFormat::Stencil8);
//a         // desc.set_vertex_function(Some(&self.vert_func));
//a         // desc.set_fragment_function(Some(&self.frag_func));
//a         // desc.set_vertex_descriptor(Some(&self.vertex_descriptor));
//a
//a         // color_attachment_desc.set_blending_enabled(true);
//a         // color_attachment_desc.set_source_rgb_blend_factor(blend_func.src_rgb);
//a         // color_attachment_desc.set_source_alpha_blend_factor(blend_func.src_alpha);
//a         // color_attachment_desc.set_destination_rgb_blend_factor(blend_func.dst_rgb);
//a         // color_attachment_desc.set_destination_alpha_blend_factor(blend_func.dst_alpha);
//a
//a         // // self.blend_func = blend_func;
//a         // // let pipeline_state = self.device.new_render_pipeline_state(&desc).unwrap();
//a         // // pipeline_state.set_label("pipeline_state");
//a         // self.pipeline_state = Some(pipeline_state);
//a
//a         // desc.set_fragment_function(None);
//a         // color_attachment_desc.set_write_mask(metal::MTLColorWriteMask::empty());
//a         // let stencil_only_pipeline_state = self.device.new_render_pipeline_state(&desc).unwrap();
//a         // // stencil_only_pipeline_state.set_label("stencil_only_pipeline_state");
//a         // self.stencil_only_pipeline_state = Some(stencil_only_pipeline_state);
//a
//a         // self.pipeline_pixel_format = pixel_format;
//a
//a         // // the rest of this function is not in metalnvg
//a         // let clear_rect_pipeline_state = {
//a         //     let desc2 = metal::RenderPipelineDescriptor::new();
//a         //     let color_attachment_desc2 = desc2.color_attachments().object_at(0).unwrap();
//a         //     color_attachment_desc2.set_pixel_format(pixel_format);
//a         //     // color_attachent_desc.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);;
//a         //     desc2.set_stencil_attachment_pixel_format(metal::MTLPixelFormat::Stencil8);
//a         //     desc2.set_fragment_function(Some(&self.clear_rect_frag_func));
//a         //     desc2.set_vertex_function(Some(&self.clear_rect_vert_func));
//a
//a         //     color_attachment_desc2.set_blending_enabled(true);
//a         //     color_attachment_desc2.set_source_rgb_blend_factor(blend_func.src_rgb);
//a         //     color_attachment_desc2.set_source_alpha_blend_factor(blend_func.src_alpha);
//a         //     color_attachment_desc2.set_destination_rgb_blend_factor(blend_func.dst_rgb);
//a         //     color_attachment_desc2.set_destination_alpha_blend_factor(blend_func.dst_alpha);
//a
//a         //     self.device.new_render_pipeline_state(&desc2).unwrap()
//a         // };
//a
//a         // // clear_rect_pipeline_state.set_label("clear_rect_pipeline_state");
//a         // self.clear_rect_pipeline_state = Some(clear_rect_pipeline_state);
//a     }
//a
//a     pub fn custom_command(
//a         &mut self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         custom_encoder: &std::sync::Arc<dyn crate::CommandEncoder>,
//a     ) {
//a         #[cfg(debug_assertions)]
//a         encoder.push_debug_group("custom_command");
//a         encoder.set_depth_stencil_state(&self.default_stencil_state);
//a
//a         custom_encoder.encode(encoder);
//a
//a         #[cfg(debug_assertions)]
//a         encoder.pop_debug_group();
//a     }
//a
//a     /// done
//a     pub fn convex_fill(
//a         &mut self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         images: &ImageStore<MtlTexture>,
//a         cmd: &Command,
//a         paint: Params,
//a     ) {
//a         #[cfg(debug_assertions)]
//a         encoder.push_debug_group("convex_fill");
//a
//a         let rps = self.current_rps.as_ref().unwrap();
//a         let pipeline_state = &rps.pipeline_state;
//a
//a         encoder.set_render_pipeline_state(&pipeline_state);
//a         self.set_uniforms(encoder, images, paint, cmd.image, cmd.alpha_mask);
//a
//a         for drawable in &cmd.drawables {
//a             if let Some((start, count)) = drawable.fill_verts {
//a                 #[cfg(debug_assertions)]
//a                 self.vertex_buffer
//a                     .add_debug_marker("convex_fill/fill buffer", start as u64..(start + count) as u64);
//a
//a                 //println!("\tconvex_fill/fill: verts #{}: start: {}, count {}", 0, start, count);
//a
//a                 // offset is in bytes
//a                 let offset = self.index_buffer.len();
//a
//a                 let byte_index_buffer_offset = offset * self.index_size;
//a                 // let byte_index_buffer_offset = start * self.index_size;
//a
//a                 // assert!(self.index_buffer.len() == start);
//a                 // triangle_fan_indices_ext(start as u32, count, &mut self.index_buffer);
//a
//a                 // original uses fans so we fake it with indices
//a                 // let indices = triangle_fan_indices_cw(start as u32, count as u32);
//a                 let triangle_fan_index_count = self
//a                     .index_buffer
//a                     .extend_with_triange_fan_indices_cw(start as u32, count as u32);
//a                 //println!("\tindex_buffer.extend_from_slice {:?}", indices);
//a                 // self.index_buffer.extend_from_slice(&indices);
//a
//a                 encoder.draw_indexed_primitives(
//a                     metal::MTLPrimitiveType::Triangle,
//a                     triangle_fan_index_count as u64, // indices.len() as u64,
//a                     metal::MTLIndexType::UInt32,
//a                     self.index_buffer.as_ref(),
//a                     byte_index_buffer_offset as u64,
//a                 );
//a             }
//a
//a             // Draw fringes
//a             if let Some((start, count)) = drawable.stroke_verts {
//a                 #[cfg(debug_assertions)]
//a                 self.vertex_buffer
//a                     .add_debug_marker("convex_fill/stroke buffer", start as u64..(start + count) as u64);
//a
//a                 //println!("\tconvex_fill/stroke: verts #{}: start: {}, count {}", 0, start, count);
//a                 encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, start as u64, count as u64)
//a             }
//a         }
//a
//a         //println!("\tconvex_fill/indices {:?}", self.index_buffer);
//a
//a         #[cfg(debug_assertions)]
//a         encoder.pop_debug_group();
//a     }
//a
//a     /// done
//a     pub fn concave_fill(
//a         &mut self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         images: &ImageStore<MtlTexture>,
//a         cmd: &Command,
//a         stencil_paint: Params,
//a         fill_paint: Params,
//a     ) {
//a         #[cfg(debug_assertions)]
//a         encoder.push_debug_group("concave_fill");
//a
//a         let rps = self.current_rps.as_ref().unwrap();
//a         let pipeline_state = &rps.pipeline_state;
//a         let stencil_only_pipeline_state = &rps.stencil_only_pipeline_state;
//a
//a         // - first we draw to the stencil buffer
//a         // - change the pipeline state to stencil
//a         // - we disable culling since fill_shape_stencil_state
//a         // does the incr/decr thing
//a         // - note that we are binding the pseudotexture
//a         //  thereby disabling writing to color buffer
//a         //
//a         encoder.set_cull_mode(metal::MTLCullMode::None);
//a         encoder.set_render_pipeline_state(stencil_only_pipeline_state);
//a         encoder.set_depth_stencil_state(&self.fill_shape_stencil_state);
//a
//a         // todo metal nanovg doesn't have this but gpucanvas does
//a         self.set_uniforms(encoder, images, stencil_paint, None, None);
//a
//a         // fill verts
//a         for drawable in &cmd.drawables {
//a             if let Some((start, count)) = drawable.fill_verts {
//a                 //println!("concave_fill/fill verts #{}: start: {}, count {}", 0, start, count);
//a                 let offset = self.index_buffer.len();
//a                 let byte_index_buffer_offset = offset * self.index_size;
//a
//a                 let triangle_fan_index_count = self
//a                     .index_buffer
//a                     .extend_with_triange_fan_indices_cw(start as u32, count as u32);
//a                 // original uses fans
//a                 encoder.draw_indexed_primitives(
//a                     metal::MTLPrimitiveType::Triangle,
//a                     triangle_fan_index_count as u64, // indices.len() as u64,
//a                     metal::MTLIndexType::UInt32,
//a                     self.index_buffer.as_ref(),
//a                     byte_index_buffer_offset as u64,
//a                 );
//a             }
//a         }
//a         // Restores states.
//a         encoder.set_cull_mode(metal::MTLCullMode::Back);
//a         encoder.set_render_pipeline_state(pipeline_state);
//a
//a         // Draws anti-aliased fragments.
//a         self.set_uniforms(encoder, images, fill_paint, cmd.image, cmd.alpha_mask);
//a
//a         // fringes
//a         if self.antialias {
//a             match cmd.fill_rule {
//a                 FillRule::NonZero => {
//a                     //gl::StencilFunc(gl::EQUAL, 0x0, 0xff),
//a                     encoder.set_depth_stencil_state(&self.fill_anti_alias_stencil_state_nonzero)
//a                 }
//a                 FillRule::EvenOdd => {
//a                     // gl::StencilFunc(gl::EQUAL, 0x0, 0x1),
//a                     encoder.set_depth_stencil_state(&self.fill_anti_alias_stencil_state_evenodd)
//a                 }
//a             }
//a
//a             for drawable in &cmd.drawables {
//a                 if let Some((start, count)) = drawable.stroke_verts {
//a                     //println!("concave_fill/stroke verts #{}: start: {}, count {}", 0, start, count);
//a                     encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, start as u64, count as u64);
//a                 }
//a             }
//a         }
//a
//a         // Draws fill.
//a         // triangle verts
//a         match cmd.fill_rule {
//a             FillRule::NonZero => {
//a                 //gl::StencilFunc(gl::NOTEQUAL, 0x0, 0xff),
//a                 encoder.set_depth_stencil_state(&self.fill_stencil_state_nonzero)
//a             }
//a             FillRule::EvenOdd => {
//a                 // gl::StencilFunc(gl::NOTEQUAL, 0x0, 0x1),
//a                 encoder.set_depth_stencil_state(&self.fill_stencil_state_evenodd)
//a             }
//a         }
//a         // encoder.set_depth_stencil_state(&self.fill_stencil_state);
//a         if let Some((start, count)) = cmd.triangles_verts {
//a             //println!("concave_fill/triangles verts #{}: start: {}, count {}", 0, start, count);
//a             encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, start as u64, count as u64);
//a         }
//a         encoder.set_depth_stencil_state(&self.default_stencil_state);
//a
//a         #[cfg(debug_assertions)]
//a         encoder.pop_debug_group();
//a     }
//a
//a     /// done
//a     pub fn stroke(
//a         &mut self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         images: &ImageStore<MtlTexture>,
//a         cmd: &Command,
//a         paint: Params,
//a     ) {
//a         #[cfg(debug_assertions)]
//a         encoder.push_debug_group("stroke");
//a
//a         self.set_uniforms(encoder, images, paint, cmd.image, cmd.alpha_mask);
//a         for drawable in &cmd.drawables {
//a             if let Some((start, count)) = drawable.stroke_verts {
//a                 encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, start as u64, count as u64)
//a             }
//a         }
//a
//a         #[cfg(debug_assertions)]
//a         encoder.pop_debug_group();
//a     }
//a
//a     /// done
//a     pub fn stencil_stroke(
//a         &mut self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         images: &ImageStore<MtlTexture>,
//a         cmd: &Command,
//a         paint1: Params,
//a         paint2: Params,
//a     ) {
//a         #[cfg(debug_assertions)]
//a         encoder.push_debug_group("stencil_stroke");
//a
//a         let rps = self.current_rps.as_ref().unwrap();
//a         let pipeline_state = &rps.pipeline_state;
//a         let stencil_only_pipeline_state = &rps.stencil_only_pipeline_state;
//a
//a         // Fills the stroke base without overlap.
//a
//a         encoder.set_depth_stencil_state(&self.stroke_shape_stencil_state);
//a         encoder.set_render_pipeline_state(pipeline_state);
//a         self.set_uniforms(encoder, images, paint2, cmd.image, cmd.alpha_mask);
//a         for drawable in &cmd.drawables {
//a             if let Some((start, count)) = drawable.stroke_verts {
//a                 encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, start as u64, count as u64)
//a             }
//a         }
//a
//a         // Draw anti-aliased pixels.
//a         self.set_uniforms(encoder, images, paint1, cmd.image, cmd.alpha_mask);
//a         encoder.set_depth_stencil_state(&self.stroke_anti_alias_stencil_state);
//a
//a         for drawable in &cmd.drawables {
//a             if let Some((start, count)) = drawable.stroke_verts {
//a                 // unsafe { gl::DrawArrays(gl::TRIANGLE_STRIP, start as i32, count as i32); }
//a                 encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, start as u64, count as u64);
//a             }
//a         }
//a
//a         // Clears stencil buffer.
//a         encoder.set_depth_stencil_state(&self.stroke_clear_stencil_state);
//a         encoder.set_render_pipeline_state(&stencil_only_pipeline_state);
//a
//a         for drawable in &cmd.drawables {
//a             if let Some((start, count)) = drawable.stroke_verts {
//a                 encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, start as u64, count as u64);
//a             }
//a         }
//a         encoder.set_depth_stencil_state(&self.default_stencil_state);
//a
//a         #[cfg(debug_assertions)]
//a         encoder.pop_debug_group();
//a     }
//a
//a     /// done
//a     pub fn triangles(
//a         &mut self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         images: &ImageStore<MtlTexture>,
//a         cmd: &Command,
//a         paint: Params,
//a     ) {
//a         #[cfg(debug_assertions)]
//a         encoder.push_debug_group("triangles");
//a
//a         let rps = self.current_rps.as_ref().unwrap();
//a         let pipeline_state = &rps.pipeline_state;
//a
//a         self.set_uniforms(encoder, images, paint, cmd.image, cmd.alpha_mask);
//a         encoder.set_render_pipeline_state(&pipeline_state);
//a         if let Some((start, count)) = cmd.triangles_verts {
//a             encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, start as u64, count as u64);
//a         }
//a
//a         #[cfg(debug_assertions)]
//a         encoder.pop_debug_group();
//a     }
//a
//a     fn set_uniforms(
//a         &self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         images: &ImageStore<MtlTexture>,
//a         paint: Params,
//a         image_tex: Option<ImageId>,
//a         alpha_tex: Option<ImageId>,
//a     ) {
//a         // println!("-------");
//a         // println!("start: set_uniforms {:?}", image_tex);
//a         // println!("uniforms render_target {:?}", self.render_target);
//a         let tex = if let Some(id) = image_tex {
//a             // if self.render_target == RenderTarget::Image(id) {
//a             // println!("render_target == image({:?}), setting pseudotexture", id);
//a             // &self.pseudo_texture
//a             // } else {
//a             // println!("render_target != image, setting id {:?}", id);
//a             images.get(id).unwrap()
//a         // }
//a         } else {
//a             // println!("image_tex == None, setting pseudo texture");
//a             &self.pseudo_texture
//a         };
//a
//a         encoder.set_fragment_texture(0, Some(&tex.tex()));
//a         encoder.set_fragment_sampler_state(0, Some(&tex.sampler()));
//a         encoder.set_fragment_value(0, &paint);
//a
//a         let mut alpha = false;
//a         let alpha_tex = if let Some(id) = alpha_tex {
//a             alpha = true;
//a             // if self.render_target == RenderTarget::Image(id) {
//a             //     &self.pseudo_texture
//a             // } else {
//a             images.get(id).unwrap()
//a         // }
//a         } else {
//a             &self.pseudo_texture
//a         };
//a
//a         // if alpha {
//a         //     encoder.push_debug_group("alpha_tex");
//a         // }
//a
//a         encoder.set_fragment_texture(1, Some(&alpha_tex.tex()));
//a         encoder.set_fragment_sampler_state(1, Some(&alpha_tex.sampler()));
//a         // println!("end: set_uniforms {:?}", image_tex);
//a         // if alpha {
//a         // encoder.pop_debug_group();
//a         // }
//a     }
//a
//a     // from warrenmoore
//a     // Well, as I think we discussed previously, scissor state doesn’t affect clear load actions in Metal, but you can simulate this by drawing a rect with a solid color with depth read disabled and depth write enabled and forcing the depth to the clear depth value (assuming you’re using a depth buffer)
//a     // Looks like in this case the depth buffer is irrelevant. Stencil buffer contents can be cleared similarly to the depth buffer, though
//a
//a     // mnvgclearwithcolor
//a     pub fn clear_rect(
//a         &mut self,
//a         encoder: &metal::RenderCommandEncoderRef,
//a         _images: &ImageStore<MtlTexture>,
//a         x: u32,
//a         y: u32,
//a         width: u32,
//a         height: u32,
//a         color: Color,
//a     ) {
//a         #[cfg(debug_assertions)]
//a         encoder.push_debug_group("clear_rect");
//a
//a         let rps = self.current_rps.as_ref().unwrap();
//a         let pipeline_state = &rps.pipeline_state;
//a         let clear_rect_pipeline_state = &rps.clear_rect_pipeline_state;
//a         // let clear_rect_stencil_state = &rps.clear_rect_stencil_state;
//a
//a         // let view_size = *self.view_size_buffer;
//a
//a         // let rect = Rect {
//a         //     x: x as _,
//a         //     // todo: this is a huge hack around the fact that i'm not sure
//a         //     // how to properly flip the y coordinate
//a         //     // it doesn't matter for single color rectangles
//a         //     y: (view_size.h as u32 - y - height) as _,
//a         //     // y: y as _,
//a         //     w: width as _,
//a         //     h: height as _,
//a         // };
//a         // let ndc_rect = rect.as_ndc((view_size.w, view_size.h));
//a
//a         // let ndc_rect1 = Rect {
//a         //     x: 2.0 * (x as f32) / view_size.w - 1.0,
//a         //     // y: (1.0 - 2.0 * (y as f32) / view_size.h),
//a         //     y: 2.0 * (y as f32) / view_size.h - 1.0,
//a         //     w: (width as f32) / view_size.w,
//a         //     h: -(height as f32) / view_size.h,
//a         // };
//a         // println!("clear_rect {:?}", ndc_rect1);
//a         let ndc_rect = Rect {
//a             x: -1.0,
//a             y: -1.0,
//a             w: 2.0,
//a             h: 2.0,
//a         };
//a         let clear_rect = ClearRect { rect: ndc_rect, color };
//a         // encoder.set_depth_stencil_state(&clear_rect_stencil_state);
//a
//a         // encoder.set_stencil_reference_value(0x00);
//a         // fill_shape_stencil_state
//a         // encoder.set_depth_stencil_state(&self.fill_shape_stencil_state);
//a         // encoder.set_stencil_reference_value(0);
//a         // encoder.set_depth_stencil_state(&self.clear_rect_stencil_state);
//a         // encoder.set_depth_stencil_state(&self.fill_anti_alias_stencil_state);
//a         // encoder.set_depth_stencil_state(&self.fill_stencil_state);
//a
//a         // encoder.set_depth_stencil_state(&self.stroke_shape_stencil_state);
//a         // encoder.set_depth_stencil_state(&self.stroke_anti_alias_stencil_state);
//a         // encoder.set_depth_stencil_state(&self.stroke_clear_stencil_state);
//a
//a         encoder.set_render_pipeline_state(clear_rect_pipeline_state);
//a         encoder.set_vertex_value(0, &clear_rect);
//a         encoder.set_scissor_rect(metal::MTLScissorRect {
//a             x: x as _,
//a             y: y as _,
//a             width: width as _,
//a             height: height as _,
//a         });
//a
//a         encoder.draw_primitives_instanced(metal::MTLPrimitiveType::TriangleStrip, 0, 4, 1);
//a
//a         // reset state
//a         // let size = *self.view_size_buffer;
//a         let size = self.view_size;
//a         encoder.set_scissor_rect(metal::MTLScissorRect {
//a             x: 0,
//a             y: 0,
//a             width: size.w as _,
//a             height: size.h as _,
//a         });
//a
//a         // reset buffers for the other commands
//a         encoder.set_render_pipeline_state(&pipeline_state);
//a         encoder.set_vertex_buffer(0, Some(self.vertex_buffer.as_ref()), 0);
//a         // encoder.set_vertex_buffer(1, Some(self.view_size_buffer.as_ref()), 0);
//a         encoder.set_vertex_value(1, &size);
//a
//a         #[cfg(debug_assertions)]
//a         encoder.pop_debug_group();
//a     }
//a
//a     pub fn set_target(&mut self, images: &ImageStore<MtlTexture>, target: RenderTarget) {
//a         if self.render_target == target {
//a             assert!(false);
//a             return;
//a         }
//a
//a         // let prev_size = self.view_size;
//a
//a         let size = match target {
//a             RenderTarget::Screen => self.layer.drawable_size().into(),
//a             RenderTarget::Image(id) => {
//a                 let texture = images.get(id).unwrap();
//a                 texture.size()
//a             }
//a             RenderTarget::None => unimplemented!("rendertarget cannot be none"),
//a         };
//a
//a         // println!(
//a         //     "frame: {:?}, setting target from {:?}({:?}) to {:?}({:?})",
//a         //     self.frame, self.render_target, prev_size, target, size
//a         // );
//a
//a         self.render_target = target;
//a
//a         self.view_size = size;
//a     }
//a
//a     // pub fn get_target(&self, images: &ImageStore<MtlTexture>) -> metal::TextureRef {
//a     //     match self.render_target {
//a     //         RenderTarget::Screen => {
//a     //             todo!()
//a     //         },
//a     //         RenderTarget::Image(id) => {
//a     //             todo!()
//a     //         }
//a     //     }
//a     // }
//a
//a     // pub fn reset(&mut self) {
//a
//a     // }
//a }
//a
//a // fn to_ndc(position: (f32, f32), drawable_size: (f32, f32)) -> (f32, f32) {
//a //     let x_scale = 2.0 / drawable_size.0;
//a //     let y_scale = 2.0 / drawable_size.1;
//a //     let x_bias = -1.0;
//a //     let y_bias = -1.0;
//a
//a //     let x = position.0 * x_scale + x_bias;
//a //     let y = position.1 * y_scale + y_bias;
//a //     (x, y)
//a // }
//a
//a // impl Rect {
//a //     pub fn as_ndc_flipped(&self, screen_size: (f32, f32)) -> Self {
//a //         let src = (self.x, self.y);
//a //         let dst = (self.x + self.w, self.y + self.h);
//a
//a //         let ndc_src = to_ndc(src, screen_size);
//a //         let ndc_dst = to_ndc(dst, screen_size);
//a
//a //         let x = ndc_src.0;
//a //         let y = ndc_src.1;
//a //         let w = ndc_dst.0 - ndc_src.0;
//a //         let h = ndc_dst.1 - ndc_src.1;
//a //         Self { x, y, w, h }
//a //     }
//a //     pub fn as_ndc(&self, screen_size: (f32, f32)) -> Self {
//a //         let src = (self.x, self.y);
//a //         // let dst = (self.x + self.w, self.y + self.h);
//a
//a //         let ndc_src = to_ndc(src, screen_size);
//a //         // let ndc_dst = to_ndc(dst, screen_size);
//a
//a //         let x = ndc_src.0;
//a //         let y = ndc_src.1;
//a //         let w = self.w / screen_size.0 * 2.0;
//a //         let h = self.h / screen_size.1 * 2.0;
//a //         Self { x, y, w, h }
//a //     }
//a // }
//a
//a impl From<Color> for metal::MTLClearColor {
//a     fn from(v: Color) -> Self {
//a         Self::new(v.r.into(), v.g.into(), v.b.into(), v.a.into())
//a     }
//a }
//a
//a static mut SHOULD_RENDER: bool = true;
//a
//a fn lock() {
//a     unsafe {
//a         SHOULD_RENDER = false;
//a     }
//a }
//a
//a fn unlock() {
//a     unsafe {
//a         SHOULD_RENDER = true;
//a     }
//a }
//a
//a fn should_render() -> bool {
//a     unsafe { SHOULD_RENDER }
//a }
//a
//a fn new_render_command_encoder<'a>(
//a     target_texture: &metal::TextureRef,
//a     command_buffer: &'a metal::CommandBufferRef,
//a     clear_color: Color,
//a     stencil_texture: &mut MtlStencilTexture,
//a     // view_size: Size,
//a     vertex_buffer: &GPUVec<Vertex>,
//a     // view_size_buffer: &GPUVar<Size>,
//a     view_size: Size,
//a     // index_buffer: &IndexBuffer,
//a     // uniform_buffer: &GPUVec<Params>,
//a     // clear_buffer_on_flush: bool,
//a ) -> &'a metal::RenderCommandEncoderRef {
//a     if true {
//a         let desc = metal::RenderPassDescriptor::new();
//a
//a         stencil_texture.resize(view_size);
//a         // let view_size = **view_size_buffer;
//a
//a         let load_action =
//a         // if clear_buffer_on_flush {
//a             metal::MTLLoadAction::Load;
//a         // } else {
//a         // metal::MTLLoadAction::Clear;
//a         // };
//a
//a         let color_attachment = desc.color_attachments().object_at(0).unwrap();
//a         color_attachment.set_clear_color(clear_color.into());
//a         color_attachment.set_load_action(load_action);
//a         color_attachment.set_store_action(metal::MTLStoreAction::Store);
//a         color_attachment.set_texture(Some(&target_texture));
//a         // added
//a
//a         let stencil_attachment = desc.stencil_attachment().unwrap();
//a         stencil_attachment.set_clear_stencil(0);
//a         stencil_attachment.set_load_action(metal::MTLLoadAction::Clear);
//a         stencil_attachment.set_store_action(metal::MTLStoreAction::DontCare);
//a         stencil_attachment.set_texture(Some(&stencil_texture.tex()));
//a
//a         let encoder = command_buffer.new_render_command_encoder(&desc);
//a
//a         encoder.set_cull_mode(metal::MTLCullMode::Back);
//a         encoder.set_front_facing_winding(metal::MTLWinding::CounterClockwise);
//a         encoder.set_stencil_reference_value(0);
//a
//a         encoder.set_viewport(metal::MTLViewport {
//a             originX: 0.0,
//a             originY: 0.0,
//a             width: view_size.w as f64,
//a             height: view_size.h as f64,
//a             znear: 0.0,
//a             zfar: 1.0,
//a         });
//a
//a         encoder.set_vertex_buffer(0, Some(vertex_buffer.as_ref()), 0);
//a         // encoder.set_vertex_buffer(1, Some(view_size_buffer.as_ref()), 0);
//a         encoder.set_vertex_value(1, &view_size);
//a         // encoder.set_fragment_buffer(0, Some(uniform_buffer.as_ref()), 0);
//a
//a         encoder
//a     } else {
//a         todo!()
//a         //     let desc = metal::RenderPassDescriptor::new();
//a         //     let color_attachment = desc.color_attachments().object_at(0).unwrap();
//a
//a         //     color_attachment.set_texture(Some(color_texture));
//a         //     color_attachment.set_load_action(metal::MTLLoadAction::Clear);
//a         //     color_attachment.set_clear_color(clear_color.into());
//a         //     color_attachment.set_store_action(metal::MTLStoreAction::Store);
//a         //     command_buffer.new_render_command_encoder(&desc)
//a     }
//a }
//a
//a impl crate::renderer::BufferCache for MtlBuffersCache {}
//a
//a impl Renderer for Mtl {
//a     type Image = MtlTexture;
//a     // type BufferCache = MtlBuffersCache;
//a     type BufferCache = crate::renderer::VoidCache;
//a
//a     fn alloc_buffer_cache(&self) -> Self::BufferCache {
//a         // Self::BufferCache::new(&self.device, self.multiple_buffering)
//a         Self::BufferCache::new()
//a     }
//a
//a     fn view_size(&self) -> Size {
//a         // *self.view_size_buffer
//a         self.view_size
//a         // match self.render_target {
//a         //     RenderTarget::Screen => {
//a         //         self.layer.drawable_size().into()
//a         //     }
//a         //     RenderTarget::Image(id) => {
//a         //         todo!()
//a         //     }
//a         // }
//a     }
//a
//a     fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
//a         let size = Size::new(width as f32, height as f32);
//a         // self.screen_view = [width as _, height as _];
//a         // *self.view_size_buffer = size;
//a         self.view_size = size;
//a     }
//a
//a     fn start_capture(&self) {
//a         let shared = metal::CaptureManager::shared();
//a         shared.start_capture_with_command_queue(&self.command_queue);
//a     }
//a
//a     fn stop_capture(&self) {
//a         let shared = metal::CaptureManager::shared();
//a         shared.stop_capture();
//a     }
//a
//a     fn label(&self, images: &ImageStore<Self::Image>, id: ImageId) -> String {
//a         let img = images.get(id).unwrap();
//a         img.label().to_owned()
//a     }
//a
//a     fn set_label(&self, images: &ImageStore<Self::Image>, id: ImageId, label: &str) {
//a         let img = images.get(id).unwrap();
//a         img.set_label(label)
//a     }
//a
//a     // called flush in ollix and nvg
//a     fn render(
//a         &mut self,
//a         images: &ImageStore<Self::Image>,
//a         cache: &mut Self::BufferCache,
//a         verts: &[Vertex],
//a         commands: &[Command],
//a     ) {
//a         //println!("verts len {:?}", verts.len());
//a         //// println!("index_buffer.byte_len {}", self.index_buffer.byte_len());
//a         //// println!("index_buffer.byte_capacity {}", self.index_buffer.byte_capacity());
//a         if !should_render() {
//a             // println!("dropping frame with target ");
//a             return;
//a         }
//a         lock();
//a
//a         #[derive(Copy, Clone, Default, Debug)]
//a         struct Counters {
//a             convex_fill: usize,
//a             concave_fill: usize,
//a             stroke: usize,
//a             stencil_stroke: usize,
//a             triangles: usize,
//a             clear_rect: usize,
//a             set_render_target: usize,
//a         }
//a
//a         let mut counters: Counters = Default::default();
//a
//a         // let lens = PathsLength::new(commands);
//a         // let max_verts = lens.vertex_count + lens.triangle_count;
//a
//a         #[cfg(debug_assertions)]
//a         self.vertex_buffer.remove_all_debug_markers();
//a
//a         self.vertex_buffer.clear();
//a         // self.index_buffer.resize(max_verts);
//a         // self.vertex_buffer.resize(verts.len());
//a         self.vertex_buffer.extend_from_slice(verts);
//a
//a         // build indices
//a         self.index_buffer.clear();
//a         self.index_buffer.resize(3 * verts.len());
//a         //// println!("reserving {}", 3 * verts.len());
//a         // temporary to ensure that the index_buffer is does not
//a         // change the inner allocation
//a         // the reserve should allocate enough
//a         let vertex_buffer_hash = self.vertex_buffer.ptr_hash();
//a         let index_buffer_hash = self.index_buffer.ptr_hash();
//a
//a         // let e = BUFFER_CACHE.acquire();
//a         // let (buffers_index, buffers) = self.buffers_cache.acquire();
//a
//a         // let mut stroke_vert_offset = max_verts - lens.stroke_count;
//a
//a         // for cmd in commands {
//a         //     for drawable in &cmd.drawables {
//a         //         if let Some((start, count)) = drawable.fill_verts {
//a         //             if count > 2 {
//a         //                 let mut hub_offset = self.vertex_buffer.len() as u32;
//a         //                 // hub_offset += 1;
//a         //                 // self.vertex_buffer.splice_slow(..2, verts[start..start+count].iter().cloned());
//a         //                 self.vertex_buffer.extend_from_slice(&verts[start..start+count]);
//a         //                 for index in 2..count {
//a         //                     self.index_buffer.extend_from_slice(&[hub_offset,
//a         //                                                 (start + index) as u32,
//a         //                                                 (start + index + 1) as u32]);
//a         //                 }
//a         //             }
//a         //         }
//a
//a         //         if let Some((start, count)) = drawable.stroke_verts {
//a         //             if count > 0 {
//a         //                 self.vertex_buffer.extend_from_slice(&verts[start..start+count]);
//a         //                 // self.vertex_buffer.splice_slow(stroke_vert_offset..stroke_vert_offset+count,
//a         //                 //     verts[start..start+count].iter().cloned());
//a         //                 //     stroke_vert_offset += count;
//a         //                 // unsafe {
//a         //                     // std::ptr::copy(
//a         //                     //     &verts[start..start+count],
//a         //                     //     self.vertex_buffer.as_mut_ptr() as _,
//a         //                     //     0
//a         //                     // );
//a
//a         //                 // }
//a
//a         //                 // ;
//a         //                 // vertex_count += count + 2;
//a         //                 // stroke_count += count;
//a         //             }
//a         //         }
//a         //     }
//a
//a         //     // if let Some((start, count)) = cmd.triangles_verts {
//a         //     //     // triangle_count += count;
//a         //     // }
//a         // }
//a
//a         let clear_color: Color = self.clear_color;
//a         //// println!("clear_color: {:?}", clear_color);
//a
//a         let command_buffer = self.command_queue.new_command_buffer().to_owned();
//a         command_buffer.enqueue();
//a         let block = block::ConcreteBlock::new(move |buffer: &metal::CommandBufferRef| {
//a             //     // println!("{}", buffer.label());
//a             // self.vertex_buffer.clear();
//a
//a             unlock();
//a         })
//a         .copy();
//a         command_buffer.add_completed_handler(&block);
//a         let mut drawable: Option<metal::MetalDrawable> = None;
//a
//a         let mut target_texture = match self.render_target {
//a             RenderTarget::Screen => {
//a                 // println!("render target: screen");
//a                 let d = self.layer.next_drawable().unwrap().to_owned();
//a                 let tex = d.texture().to_owned();
//a                 drawable = Some(d);
//a                 tex
//a             }
//a             RenderTarget::Image(id) => {
//a                 // println!("render target: image: {:?}", id);
//a                 images.get(id).unwrap().tex().to_owned()
//a             }
//a             RenderTarget::None => unimplemented!("rendertarget cannot be none"),
//a         };
//a
//a         // this is needed for screenshotting
//a         // self.last_rendered_texture = Some(color_texture.to_owned());
//a
//a         let size = Size::new(target_texture.width() as _, target_texture.height() as _);
//a         // println!("target_texture size: {:?}", size);
//a         // println!("pre stencil texture size: {:?}", self.stencil_texture.size());
//a
//a         // println!("pre stencil texture size: {:?}", self.stencil_texture.size());
//a         // assert_eq!(size, *self.view_size_buffer);
//a         let mut encoder = new_render_command_encoder(
//a             &target_texture,
//a             &command_buffer,
//a             clear_color,
//a             &mut self.stencil_texture,
//a             &self.vertex_buffer,
//a             // &self.view_size_buffer,
//a             self.view_size
//a             // &self.uniform_buffer,
//a             // self.clear_buffer_on_flush,
//a         );
//a
//a         encoder.push_debug_group(&format!("frame: {:?}", self.frame));
//a
//a         let mut pixel_format = target_texture.pixel_format();
//a         encoder.push_debug_group(&format!("target: {:?}", self.render_target));
//a         // let mut data: Vec<u8> = Vec::with_capacity(500 * 500);
//a         // for data in data.iter_mut() {
//a         //     *data = 0x1;
//a         // }
//a         // match self.render_target {
//a         //     RenderTarget::Screen => {
//a         //         encoder.push_debug_group("rendering to screen");
//a         //     }
//a         //     RenderTarget::Image(id) => {
//a         //         encoder.push_debug_group(&format!("rendering to image: {:?}", id));
//a         //     }
//a         // }
//a         // self.stencil_texture.resize();
//a         // self.clear_buffer_on_flush = false;p
//a
//a         // pub struct RenderContext {}
//a
//a         // let mut switches = vec![];
//a
//a         // for (i, cd) in commands.iter().enumerate() {
//a
//a         // }
//a
//a         // fn dump_command_type(cmd: &Command) -> &str {
//a         //     match cmd.cmd_type {
//a         //         CommandType::ConvexFill { .. } => "convex_fill",
//a         //         CommandType::ConcaveFill { .. } => "concave_fill",
//a         //         CommandType::Stroke { .. } => "stroke",
//a         //         CommandType::StencilStroke { .. } => "stencil_stroke",
//a         //         CommandType::Triangles { .. } => "triangles",
//a         //         CommandType::ClearRect { .. } => "clear_rect",
//a         //         CommandType::SetRenderTarget { .. } => "set_render_target",
//a         //     }
//a         // }
//a         // println!("loop start");
//a         let mut target_set = 0;
//a         for cmd in commands {
//a             // println!("command_type: {:?}", dump_command_type(cmd));
//a             self.set_composite_operation(cmd.composite_operation, pixel_format);
//a
//a             match &cmd.cmd_type {
//a                 CommandType::Blit {
//a                     source,
//a                     destination_origin,
//a                 } => {
//a                     // encoder.pop_debug_group();
//a                     // encoder.end_encoding();
//a
//a                     // let destination_origin = metal::MTLOrigin {
//a                     //     x: destination_origin.0 as _,
//a                     //     y: destination_origin.1 as _,
//a                     //     z: 0,
//a                     // };
//a
//a                     // let blit_encoder = command_buffer.new_blit_command_encoder();
//a                     // let source_texture = images.get(*source).map(|x| x.tex()).unwrap();
//a
//a                     // let destination_texture = match self.render_target {
//a                     //     RenderTarget::None => todo!(),
//a                     //     RenderTarget::Screen => {
//a                     //         // self.layer.next_drawable().unwrap().texture()
//a                     //         self.layer.next_drawable().and_then(|x| Some(x.texture()))
//a                     //     }
//a                     //     RenderTarget::Image(id) => {
//a                     //         // images.get(id).unwrap().tex()
//a                     //         images.get(id).and_then(|x| Some(x.tex()))
//a                     //         // todo!()
//a                     //     }
//a                     // }
//a                     // .unwrap();
//a                     // blit_encoder.blit(source_texture, destination_texture, destination_origin);
//a                     // blit_encoder.synchronize_resource(&destination_texture);
//a                     // blit_encoder.end_encoding();
//a
//a                     // encoder = new_render_command_encoder(
//a                     //     &target_texture,
//a                     //     &command_buffer,
//a                     //     clear_color,
//a                     //     &mut self.stencil_texture,
//a                     //     &self.vertex_buffer,
//a                     //     // &self.view_size_buffer,
//a                     //     self.view_size,
//a                     //     // &self.uniform_buffer,
//a                     //     // self.clear_buffer_on_flush,
//a                     // );
//a                     // // encoder.push_debug_group(&format!("target: {:?}", target));
//a                     // encoder.push_debug_group("unknown debug group from blit");
//a                     todo!("blit is not implemented");
//a                 }
//a                 CommandType::GPUTriangle => {
//a                     self.gpu_encoder.encode(encoder);
//a                 }
//a                 CommandType::CustomCommand { command_encoder } => {
//a                     // encoder.pop_debug_group();
//a                     // encoder.end_encoding();
//a
//a                     // let texture = match self.render_target {
//a                     //     RenderTarget::None => todo!(),
//a                     //     RenderTarget::Screen => {
//a                     //         // self.layer.next_drawable().unwrap().texture()
//a                     //         self.layer.next_drawable().and_then(|x| Some(x.texture()))
//a                     //     }
//a                     //     RenderTarget::Image(id) => {
//a                     //         // images.get(id).unwrap().tex()
//a                     //         images.get(id).and_then(|x| Some(x.tex()))
//a                     //         // todo!()
//a                     //     }
//a                     // }
//a                     // .unwrap();
//a
//a                     // encoder = new_render_command_encoder(
//a                     //     &texture,
//a                     //     &command_buffer,
//a                     //     clear_color,
//a                     //     &mut self.stencil_texture,
//a                     //     &self.vertex_buffer,
//a                     //     // &self.view_size_buffer,
//a                     //     self.view_size,
//a                     //     // &self.uniform_buffer,
//a                     //     // self.clear_buffer_on_flush,
//a                     // );
//a
//a                     // let width = 500;
//a                     // let height = 500;
//a                     // let reg = metal::MTLRegion::new_2d(0, 0, width as _, height as _);
//a                     // self.stencil_texture
//a                     //     .tex()
//a                     //     .replace_region(reg, 0, data.as_ptr() as _, (width) as u64);
//a
//a                     {
//a                         #[cfg(debug_assertions)]
//a                         encoder.push_debug_group("custom_command");
//a                         // // encoder.set_depth_stencil_state(&self.default_stencil_state);
//a
//a                         command_encoder.encode(encoder);
//a
//a                         #[cfg(debug_assertions)]
//a                         encoder.pop_debug_group();
//a                     }
//a                     if false {
//a                         #[cfg(debug_assertions)]
//a                         encoder.push_debug_group("clear_stencil");
//a
//a                         encoder.set_render_pipeline_state(&self.clear_stencil_rps);
//a                         // encoder.set_depth_stencil_state(&self.fill_shape_stencil_state);
//a                         encoder.set_depth_stencil_state(&self.default_stencil_state);
//a                         encoder.set_cull_mode(metal::MTLCullMode::None);
//a
//a                         // let clear_coords = [-1, -1, 1, -1, -1, 1, 1, 1];
//a                         let clear_coords: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
//a                         // let clear_coords: [f32; 8] = [-0.5, -0.5, 0.5, -0.5, -0.5, 0.5, 0.5, 0.5];
//a                         // let clear_coords: [f32; 8] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
//a                         encoder.set_vertex_bytes(0, (8 * std::mem::size_of::<f32>()) as _, clear_coords.as_ptr() as _);
//a                         encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
//a
//a                         encoder.set_cull_mode(metal::MTLCullMode::Back);
//a                         #[cfg(debug_assertions)]
//a                         encoder.pop_debug_group();
//a                     }
//a                     // encoder.end_encoding();
//a
//a                     // encoder = new_render_command_encoder(
//a                     //     &texture,
//a                     //     &command_buffer,
//a                     //     clear_color,
//a                     //     &mut self.stencil_texture,
//a                     //     &self.vertex_buffer,
//a                     //     // &self.view_size_buffer,
//a                     //     self.view_size,
//a                     //     // &self.uniform_buffer,
//a                     //     // self.clear_buffer_on_flush,
//a                     // );
//a                     // self.custom_command(encoder, &command_encoder);
//a                     // command_encoder.encode(encoder);
//a                 }
//a                 CommandType::ConvexFill { params } => {
//a                     counters.convex_fill += 1;
//a                     self.convex_fill(&encoder, images, cmd, *params)
//a                 }
//a                 CommandType::ConcaveFill {
//a                     stencil_params,
//a                     fill_params,
//a                 } => {
//a                     counters.concave_fill += 1;
//a                     self.concave_fill(&encoder, images, cmd, *stencil_params, *fill_params)
//a                 }
//a                 CommandType::Stroke { params } => {
//a                     counters.stroke += 1;
//a                     self.stroke(&encoder, images, cmd, *params)
//a                 }
//a                 CommandType::StencilStroke { params1, params2 } => {
//a                     counters.stencil_stroke += 1;
//a                     self.stencil_stroke(&encoder, images, cmd, *params1, *params2)
//a                 }
//a                 CommandType::Triangles { params } => {
//a                     counters.triangles += 1;
//a                     self.triangles(&encoder, images, cmd, *params)
//a                 }
//a                 CommandType::ClearRect {
//a                     x,
//a                     y,
//a                     width,
//a                     height,
//a                     color,
//a                 } => {
//a                     counters.clear_rect += 1;
//a                     self.clear_rect(&encoder, images, *x, *y, *width, *height, *color);
//a                 }
//a                 CommandType::SetRenderTarget(target) => {
//a                     if self.render_target == *target {
//a                         // println!("skipping target setting");
//a                         continue;
//a                     }
//a                     encoder.pop_debug_group();
//a
//a                     target_set += 1;
//a                     counters.set_render_target += 1;
//a                     // println!("---------switching from {:?} to {:?}", self.render_target, target);
//a
//a                     encoder.end_encoding();
//a                     self.set_target(images, *target);
//a
//a                     // if let Some(drawable) = drawable.as_ref() {
//a                     //     command_buffer.present_drawable(&drawable);
//a                     // }
//a
//a                     let size;
//a                     target_texture = match self.render_target {
//a                         RenderTarget::Screen => {
//a                             // println!("render target: screen");
//a                             let d = self.layer.next_drawable().unwrap().to_owned();
//a                             let tex = d.texture().to_owned();
//a                             drawable = Some(d);
//a                             size = self.layer.drawable_size().into();
//a                             tex
//a                         }
//a                         RenderTarget::Image(id) => {
//a                             // println!("render target: image: {:?}", id);
//a                             let tex = images.get(id).unwrap().tex().to_owned();
//a                             size = tex.size();
//a                             tex
//a                         }
//a                         RenderTarget::None => unimplemented!("rendertarget cannot be none"),
//a                     };
//a                     pixel_format = target_texture.pixel_format();
//a                     // println!("size0: {:?}, size1: {:?}", size, *self.view_size_buffer);
//a                     // assert!(size == *self.view_size_buffer);
//a                     assert!(size == self.view_size);
//a
//a                     encoder = new_render_command_encoder(
//a                         &target_texture,
//a                         &command_buffer,
//a                         clear_color,
//a                         &mut self.stencil_texture,
//a                         &self.vertex_buffer,
//a                         // &self.view_size_buffer,
//a                         self.view_size,
//a                         // &self.uniform_buffer,
//a                         // self.clear_buffer_on_flush,
//a                     );
//a                     encoder.push_debug_group(&format!("target: {:?}", target));
//a                 }
//a             }
//a         }
//a         // println!("loop end");
//a         // pop the target debug group
//a         encoder.pop_debug_group();
//a         // pop the frame debug group
//a         encoder.pop_debug_group();
//a
//a         encoder.end_encoding();
//a         // it appears that having this print statement influences things
//a         // println!("target_set: {:?}", target_set);
//a
//a         // println!("pre present");
//a         if let Some(drawable) = drawable {
//a             command_buffer.present_drawable(&drawable);
//a         }
//a
//a         // println!("target set {}", target_set);
//a         // println!("pre blit");
//a         // Makes mnvgReadPixels-like functions (e.g. screenshot) work as expected on Mac.
//a         #[cfg(target_os = "macos")]
//a         {
//a             if self.render_target == RenderTarget::Screen {
//a                 let blit = command_buffer.new_blit_command_encoder();
//a                 blit.synchronize_resource(&target_texture);
//a                 blit.end_encoding();
//a             }
//a         }
//a
//a         // println!("post blit");
//a         command_buffer.commit();
//a
//a         // println!("post commit");
//a         // if self.frame == 1 {
//a         //     color_texture.save_to("/Users/adamnemecek/Code/ngrid/main/vendor/ngrid10deps/gpucanvas/out.png");
//a         // }
//a         self.frame += 1;
//a
//a         assert!(vertex_buffer_hash == self.vertex_buffer.ptr_hash());
//a         assert!(index_buffer_hash == self.index_buffer.ptr_hash());
//a
//a         // command_buffer.wait_until_scheduled();
//a         // println!("counters {:?}", counters);
//a
//a         // if !self.layer.presents_with_transaction() {
//a         //     command_buffer.present_drawable(&drawable);
//a         // }
//a
//a         // if self.layer.presents_with_transaction() {
//a         //     command_buffer.wait_until_scheduled();
//a         //     // drawable.present();
//a         // }
//a     }
//a
//a     fn alloc_image(&mut self, info: ImageInfo) -> Result<Self::Image, ErrorKind> {
//a         let mut img = Self::Image::new(&self.device, &self.command_queue, info);
//a         img.as_mut().unwrap().set_clear_color(rgb::RGBA8 {
//a             r: 255,
//a             g: 255,
//a             b: 255,
//a             a: 255,
//a         });
//a         img
//a     }
//a
//a     fn update_image(
//a         &mut self,
//a         image: &mut Self::Image,
//a         data: ImageSource,
//a         x: usize,
//a         y: usize,
//a     ) -> Result<(), ErrorKind> {
//a         image.update(data, x, y)
//a     }
//a
//a     fn delete_image(&mut self, image: Self::Image) {
//a         image.delete();
//a     }
//a
//a     // fn set_target(&mut self, images: &ImageStore<MtlTexture>, target: RenderTarget) {
//a     //     self.render_target = target;
//a     //     todo!();
//a     // }
//a
//a     fn flip_y() -> bool {
//a         true
//a     }
//a
//a     fn flip_uv() -> bool {
//a         true
//a     }
//a
//a     fn screenshot(&mut self, images: &ImageStore<Self::Image>) -> Result<ImgVec<RGBA8>, ErrorKind> {
//a         println!("screenshot: {:?}", self.render_target);
//a
//a         let texture = match self.render_target {
//a             RenderTarget::Screen => self.layer.next_drawable().map(|x| x.texture()),
//a             RenderTarget::Image(id) => images.get(id).map(|x| x.tex()),
//a             RenderTarget::None => unimplemented!("rendertarget cannot be none"),
//a         }
//a         .unwrap();
//a         // let texture = self.last_rendered_texture.as_ref().unwrap();
//a
//a         // todo!()
//a         // look at headless renderer in metal-rs
//a         // let size = *self.view_size_buffer;
//a         let width = texture.width();
//a         let height = texture.height();
//a         let w = width as u64;
//a         let h = height as u64;
//a
//a         let mut buffer = ImgVec::new(
//a             vec![
//a                 RGBA8 {
//a                     r: 255,
//a                     g: 255,
//a                     b: 255,
//a                     a: 255
//a                 };
//a                 (w * h) as usize
//a             ],
//a             w as usize,
//a             h as usize,
//a         );
//a
//a         texture.get_bytes(
//a             buffer.buf_mut().as_ptr() as *mut std::ffi::c_void,
//a             w * 4,
//a             metal::MTLRegion {
//a                 origin: metal::MTLOrigin::default(),
//a                 size: metal::MTLSize {
//a                     width: w,
//a                     height: h,
//a                     depth: 1,
//a                 },
//a             },
//a             0,
//a         );
//a
//a         Ok(buffer)
//a     }
//a }
