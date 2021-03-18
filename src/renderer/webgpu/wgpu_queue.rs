#[derive(Clone)]
pub struct WGPUContext {
    device: std::rc::Rc<wgpu::Device>,
    queue: std::rc::Rc<wgpu::Queue>,
}

impl WGPUContext {
    pub fn device(&self) -> &std::rc::Rc<wgpu::Device> {
        &self.device
    }

    pub fn queue(&self) -> &std::rc::Rc<wgpu::Queue> {
        todo!()
    }
}
// #[derive(Clone)]
// pub struct WGPUDevice {
//     inner: std::rc::Rc<wgpu::Device>
// }
