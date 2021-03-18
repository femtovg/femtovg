
#[derive(Clone)]
pub struct WGPUContext {
    device: std::rc::Rc<wgpu::Device>,
    queue: std::rc::Rc<wgpu::Queue>,
}

// #[derive(Clone)]
// pub struct WGPUDevice {
//     inner: std::rc::Rc<wgpu::Device>
// }
