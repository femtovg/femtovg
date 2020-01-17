#![allow(unused_variables)]

use fnv::FnvHashMap;
use image::{
    Rgba,
    RgbaImage,
    DynamicImage,
    GenericImage,
    Pixel,
    ImageBuffer,
    imageops::{self, FilterType},
};

use crate::{Verb, Color, Paint, Scissor};
use super::{Renderer, ImageId, Vertex, ImageFlags};

pub struct ImageRenderer {
    image: RgbaImage,
    current_path: Option<Vec<Verb>>,
    last_image_id: u32,
    images: FnvHashMap<ImageId, DynamicImage>
}

impl ImageRenderer {
    pub fn new() -> Self {
        Self {
            image: RgbaImage::new(1,1),
            current_path: None,
            last_image_id: Default::default(),
            images: Default::default()
        }
    }
}

impl Renderer for ImageRenderer {
    fn flush(&mut self) {}

    fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let p = Rgba::from_channels((color.r * 255.0) as u8, (color.g * 255.0) as u8, (color.b * 255.0) as u8, (color.a * 255.0) as u8);
        let new = ImageBuffer::from_pixel(width, height, p);
        let _ = self.image.copy_from(&new, x, y);
    }

    fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
        self.image = imageops::resize(&self.image, width, height, FilterType::Nearest);
    }

    fn set_current_path(&mut self, verbs: &[Verb]) {
        if self.current_path.is_none() {
            self.current_path = Some(verbs.to_owned());
        }
    }

    fn clear_current_path(&mut self) {
        self.current_path = None;
    }

    fn fill(&mut self, paint: &Paint, scissor: &Scissor) {
        let path = if let Some(path) = self.current_path.as_ref() {
            path
        } else {
            return;
        };

        // Maybe some day
    }

    fn stroke(&mut self, paint: &Paint, scissor: &Scissor) {
        let path = if let Some(path) = self.current_path.as_ref() {
            path
        } else {
            return;
        };

        // Maybe some day
    }

    fn triangles(&mut self, paint: &Paint, scissor: &Scissor, verts: &[Vertex]) {
        // Maybe some day
    }

    fn create_image(&mut self, image: &DynamicImage, flags: ImageFlags) -> ImageId {
        let id = self.last_image_id;
        self.last_image_id = self.last_image_id.wrapping_add(1);

        self.images.insert(ImageId(id), image.clone());

        ImageId(id)
    }

    fn update_image(&mut self, id: ImageId, subimage: &DynamicImage, x: u32, y: u32) {
        let image = match self.images.get_mut(&id) {
            Some(image) => image,
            None => return
        };

        let _ = image.copy_from(subimage, x, y);
    }

    fn delete_image(&mut self, id: ImageId) {
        self.images.remove(&id);
    }

    fn screenshot(&mut self) -> Option<DynamicImage> {
        Some(DynamicImage::ImageRgba8(self.image.clone()))
    }
}
