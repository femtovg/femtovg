use super::WGPUContext;

pub struct WGPUVec<T> {
    cpu: Vec<T>,
    gpu: wgpu::Buffer,
    ph: std::marker::PhantomData<T>,
}

impl<T> WGPUVec<T> {
    pub fn new(queue: WGPUContext) -> Self {
        // Self {
        //     cpu: vec![],
        //     gpu:
        // }
        todo!()
    }
    pub fn sync(&mut self) {}
}
