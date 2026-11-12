//! The headless wgpu device, the canvas that draws into an offscreen target, and the frame's readback to PNG.

use std::time::{Duration, Instant};

use femtovg::renderer::WGPURenderer;

use crate::scene::{Canvas, Painter, SceneOptions};

/// Canvas settings beyond the scene's.
#[derive(Clone, Copy, Debug)]
pub struct CanvasOptions {
    /// Lifts the library's transient-image budget, so a conformance run measures rendering, not the budget.
    pub transient_budget_mb: usize,
    /// The filter work budget in Gi samples; the library's default when `None`.
    pub filter_work_gsamples: Option<u64>,
    /// Lowers the device's texture dimension limit, to model a small GPU.
    pub max_texture_size: Option<u32>,
}

impl Default for CanvasOptions {
    fn default() -> Self {
        Self {
            transient_budget_mb: 1024,
            filter_work_gsamples: None,
            max_texture_size: None,
        }
    }
}

/// How long one frame took: recording and encoding it on the CPU (the scene walk and the flush), and the whole
/// frame to the GPU's completion.
#[derive(Clone, Copy, Debug)]
pub struct FrameTime {
    pub encode: Duration,
    pub wall: Duration,
}

/// One persistent canvas drawing into an offscreen RGBA8 target.
pub struct Session {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub canvas: Canvas,
    pub painter: Painter,
    target: wgpu::Texture,
    size: (u32, u32),
    /// Transient image bytes held at the last flush.
    pub transient_bytes: usize,
}

impl Session {
    pub fn new(scene: SceneOptions, options: CanvasOptions) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .map_err(|e| format!("no GPU adapter: {e}"))?;
        let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        if let Some(max) = options.max_texture_size {
            limits.max_texture_dimension_2d = max;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits,
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU device: {e}"))?;

        let size = scene.frame;
        let painter = Painter::new(scene);
        let renderer = WGPURenderer::new(device.clone(), queue.clone());
        let mut canvas =
            Canvas::new_with_text_context(renderer, painter.text_context()).map_err(|e| format!("canvas: {e:?}"))?;
        canvas.set_size(size.0, size.1, 1.0);
        canvas.set_transient_image_budget(options.transient_budget_mb << 20);
        if let Some(gs) = options.filter_work_gsamples {
            canvas.set_filter_work_budget(gs << 30);
        }
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("svg-harness target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Ok(Self {
            device,
            queue,
            canvas,
            painter,
            target,
            size,
            transient_bytes: 0,
        })
    }

    /// Draws one frame of `tree` at `zoom` and waits for the GPU to finish it.
    pub fn draw(&mut self, tree: &usvg::Tree, zoom: f32) -> FrameTime {
        let started = Instant::now();
        self.painter.render(&mut self.canvas, tree, zoom);
        self.transient_bytes = self.canvas.transient_image_bytes();
        let commands = self.canvas.flush_to_output(&self.target);
        let encode = started.elapsed();
        self.queue.submit(commands);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device lost");
        FrameTime {
            encode,
            wall: started.elapsed(),
        }
    }

    /// Render passes and command buffers of the last frame.
    pub fn render_passes(&self) -> (u32, usize) {
        let slices = self.canvas.renderer().last_frame_slices();
        (slices.iter().sum(), slices.len())
    }

    /// Copies the target back as tightly packed RGBA8 rows.
    pub fn read_pixels(&self) -> Vec<u8> {
        let (width, height) = self.size;
        let unpadded = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("svg-harness readback"),
            size: u64::from(padded * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device lost");
        let mapped = slice.get_mapped_range().expect("readback mapped");
        mapped
            .chunks(padded as usize)
            .flat_map(|row| &row[..unpadded as usize])
            .copied()
            .collect()
    }

    /// Writes the last frame to `path` as an RGBA8 PNG.
    pub fn save_png(&self, path: &str) -> Result<(), String> {
        let (width, height) = self.size;
        image::save_buffer(path, &self.read_pixels(), width, height, image::ColorType::Rgba8)
            .map_err(|e| format!("{path}: {e}"))
    }
}
