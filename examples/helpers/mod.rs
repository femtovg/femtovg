use super::run;

mod perf_graph;
#[allow(unused_imports)]
pub use perf_graph::PerfGraph;

pub trait WindowSurface {
    type Renderer: femtovg::Renderer + 'static;
    fn resize(&mut self, width: u32, height: u32);
    fn present(&self, canvas: &mut femtovg::Canvas<Self::Renderer>);
}

#[cfg(not(feature = "wgpu"))]
mod opengl;

#[cfg(feature = "wgpu")]
mod wgpu;

pub fn start(
    #[cfg(not(target_arch = "wasm32"))] width: u32,
    #[cfg(not(target_arch = "wasm32"))] height: u32,
    #[cfg(not(target_arch = "wasm32"))] title: &'static str,
    #[cfg(not(target_arch = "wasm32"))] resizeable: bool,
) {
    #[cfg(not(feature = "wgpu"))]
    use opengl::start_opengl as async_start;
    #[cfg(feature = "wgpu")]
    use wgpu::start_wgpu as async_start;
    #[cfg(not(target_arch = "wasm32"))]
    spin_on::spin_on(async_start(width, height, title, resizeable));
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async_start());
}
