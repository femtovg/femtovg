use bitflags::bitflags;
use imgref::*;
use rgb::alt::Gray;
use rgb::*;
use slotmap::{DefaultKey, SlotMap};

#[cfg(feature = "image-loading")]
use ::image::DynamicImage;

#[cfg(feature = "image-loading")]
use std::convert::TryFrom;

use crate::{ErrorKind, Renderer};

/// An image handle.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(DefaultKey);

/// Specifies the format of an image's pixels.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PixelFormat {
    /// 24-bit RGB image format (8 bits per channel)
    Rgb8,
    /// 32-bit RGBA image format (8 bits per channel, including alpha)
    Rgba8,
    /// 8-bit grayscale image format
    Gray8,
}

bitflags! {
    /// Represents a set of flags that modify the behavior of an image.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ImageFlags: u32 {
        /// Generates mipmaps during the creation of the image.
        const GENERATE_MIPMAPS = 1;
        /// Repeats the image in the X direction when rendered.
        const REPEAT_X = 1 << 1;
        /// Repeats the image in the Y direction when rendered.
        const REPEAT_Y = 1 << 2;
        /// Flips (inverses) the image in the Y direction when rendered.
        const FLIP_Y = 1 << 3;
        /// Indicates that the image data has premultiplied alpha.
        const PREMULTIPLIED = 1 << 4;
        /// Uses nearest-neighbor interpolation instead of linear interpolation when rendering the image.
        const NEAREST = 1 << 5;
    }
}

/// Represents the source of an image.
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub enum ImageSource<'a> {
    /// Image source with RGB image format (8 bits per channel)
    Rgb(ImgRef<'a, RGB8>),
    /// Image source with RGBA image format (8 bits per channel, including alpha)
    Rgba(ImgRef<'a, RGBA8>),
    /// Image source with 8-bit grayscale image format
    Gray(ImgRef<'a, Gray<u8>>),
    /// Image source referencing a HTML image element (only available on `wasm32` target)
    #[cfg(target_arch = "wasm32")]
    HtmlImageElement(&'a web_sys::HtmlImageElement),
    /// Image source referencing a HTML canvas element (only available on `wasm32` target)
    #[cfg(target_arch = "wasm32")]
    HtmlCanvasElement(&'a web_sys::HtmlCanvasElement),
}

impl ImageSource<'_> {
    /// Returns the format of the image source.
    pub fn format(&self) -> PixelFormat {
        match self {
            Self::Rgb(_) => PixelFormat::Rgb8,
            Self::Rgba(_) => PixelFormat::Rgba8,
            Self::Gray(_) => PixelFormat::Gray8,
            #[cfg(target_arch = "wasm32")]
            Self::HtmlImageElement(_) | Self::HtmlCanvasElement(_) => PixelFormat::Rgba8,
        }
    }

    /// Returns the dimensions (width and height) of the image source.
    pub fn dimensions(&self) -> Size {
        match self {
            Self::Rgb(imgref) => Size::new(imgref.width(), imgref.height()),
            Self::Rgba(imgref) => Size::new(imgref.width(), imgref.height()),
            Self::Gray(imgref) => Size::new(imgref.width(), imgref.height()),
            #[cfg(target_arch = "wasm32")]
            Self::HtmlImageElement(element) => Size::new(element.width() as usize, element.height() as usize),
            #[cfg(target_arch = "wasm32")]
            Self::HtmlCanvasElement(element) => Size::new(element.width() as usize, element.height() as usize),
        }
    }

    /// Checks that this source may be copied into the image described by `info`
    /// with its top left corner at (`x`, `y`).
    ///
    /// Every [`Renderer::update_image`](crate::Renderer::update_image)
    /// implementation should call this before touching the graphics API. A copy
    /// that reaches past the destination, or that carries a pixel format the
    /// destination was not created with, is a caller mistake and has to be
    /// reported as [`ErrorKind::ImageUpdateOutOfBounds`] or
    /// [`ErrorKind::ImageUpdateWithDifferentFormat`]. Passing one down to the
    /// graphics API instead produces a driver side validation failure, which
    /// backends are generally not able to turn back into a recoverable error:
    /// the usual outcome is an abort that takes the whole process, or on wasm
    /// the whole application, rather than the single failed call.
    pub fn check_update(&self, info: &ImageInfo, x: usize, y: usize) -> Result<(), ErrorKind> {
        let size = self.dimensions();

        // Saturating, so that an origin close to `usize::MAX` reports the error
        // rather than wrapping into a range that looks valid.
        if x.saturating_add(size.width) > info.width() || y.saturating_add(size.height) > info.height() {
            return Err(ErrorKind::ImageUpdateOutOfBounds);
        }

        if info.format() != self.format() {
            return Err(ErrorKind::ImageUpdateWithDifferentFormat);
        }

        Ok(())
    }
}

impl<'a> From<ImgRef<'a, RGB8>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, RGB8>) -> Self {
        Self::Rgb(src)
    }
}

impl<'a> From<ImgRef<'a, RGBA8>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, RGBA8>) -> Self {
        Self::Rgba(src)
    }
}

impl<'a> From<ImgRef<'a, Gray<u8>>> for ImageSource<'a> {
    fn from(src: ImgRef<'a, Gray<u8>>) -> Self {
        Self::Gray(src)
    }
}

#[cfg(target_arch = "wasm32")]
impl<'a> From<&'a web_sys::HtmlImageElement> for ImageSource<'a> {
    fn from(src: &'a web_sys::HtmlImageElement) -> Self {
        Self::HtmlImageElement(src)
    }
}

#[cfg(target_arch = "wasm32")]
impl<'a> From<&'a web_sys::HtmlCanvasElement> for ImageSource<'a> {
    fn from(src: &'a web_sys::HtmlCanvasElement) -> Self {
        Self::HtmlCanvasElement(src)
    }
}

#[cfg(feature = "image-loading")]
impl<'a> TryFrom<&'a DynamicImage> for ImageSource<'a> {
    type Error = ErrorKind;

    fn try_from(src: &'a DynamicImage) -> Result<Self, ErrorKind> {
        Ok(match src {
            ::image::DynamicImage::ImageLuma8(img) => {
                let src: Img<&[Gray<u8>]> = Img::new(img.as_pixels(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            ::image::DynamicImage::ImageRgb8(img) => {
                let src = Img::new(img.as_rgb(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            ::image::DynamicImage::ImageRgba8(img) => {
                let src = Img::new(img.as_rgba(), img.width() as usize, img.height() as usize);
                ImageSource::from(src)
            }
            // TODO: if format is not supported maybe we should convert it here,
            // But that is an expensive operation on the render thread that will remain hidden from the user
            _ => return Err(ErrorKind::UnsupportedImageFormat),
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

/// Information about an image.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImageInfo {
    flags: ImageFlags,
    size: Size,
    format: PixelFormat,
}

impl ImageInfo {
    /// Creates a new `ImageInfo` with the specified flags, width, height, and format.
    pub fn new(flags: ImageFlags, width: usize, height: usize, format: PixelFormat) -> Self {
        Self {
            flags,
            size: Size { width, height },
            format,
        }
    }

    /// Returns the image flags.
    pub fn flags(&self) -> ImageFlags {
        self.flags
    }

    /// Returns the image width in pixels.
    pub fn width(&self) -> usize {
        self.size.width
    }

    /// Returns the image height in pixels.
    pub fn height(&self) -> usize {
        self.size.height
    }

    /// Returns the image size (width and height) in pixels.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the image format.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// Sets the image format.
    pub fn set_format(&mut self, format: PixelFormat) {
        self.format = format;
    }
}

#[derive(Debug)]
pub struct ImageStore<T>(SlotMap<DefaultKey, (ImageInfo, T)>);

impl<T> Default for ImageStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ImageStore<T> {
    pub fn new() -> Self {
        Self(SlotMap::new())
    }

    pub fn alloc<R: Renderer<Image = T>>(&mut self, renderer: &mut R, info: ImageInfo) -> Result<ImageId, ErrorKind> {
        let image = renderer.alloc_image(info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    pub fn register_native_texture<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        texture: R::NativeTexture,
        info: ImageInfo,
    ) -> Result<ImageId, ErrorKind> {
        let image = renderer.create_image_from_native_texture(texture, info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    pub fn register_external_texture<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        texture: R::ExternalTexture,
        info: ImageInfo,
    ) -> Result<ImageId, ErrorKind> {
        let image = renderer.create_image_from_external_texture(texture, info)?;
        Ok(ImageId(self.0.insert((info, image))))
    }

    // Reallocates the image without changing the id.
    pub fn realloc<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        id: ImageId,
        info: ImageInfo,
    ) -> Result<(), ErrorKind> {
        if let Some(old) = self.0.get_mut(id.0) {
            let new = renderer.alloc_image(info)?;
            old.0 = info;
            old.1 = new;
            Ok(())
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
    }

    pub fn get(&self, id: ImageId) -> Option<&T> {
        self.0.get(id.0).map(|inner| &inner.1)
    }

    pub fn get_mut(&mut self, id: ImageId) -> Option<&mut T> {
        self.0.get_mut(id.0).map(|inner| &mut inner.1)
    }

    pub fn update<R: Renderer<Image = T>>(
        &mut self,
        renderer: &mut R,
        id: ImageId,
        data: ImageSource,
        x: usize,
        y: usize,
    ) -> Result<(), ErrorKind> {
        if let Some(image) = self.0.get_mut(id.0) {
            renderer.update_image(&mut image.1, data, x, y)?;
            Ok(())
        } else {
            Err(ErrorKind::ImageIdNotFound)
        }
    }

    pub fn info(&self, id: ImageId) -> Option<ImageInfo> {
        self.0.get(id.0).map(|inner| inner.0)
    }

    pub fn remove<R: Renderer<Image = T>>(&mut self, renderer: &mut R, id: ImageId) {
        if let Some(image) = self.0.remove(id.0) {
            renderer.delete_image(image.1, id);
        }
    }

    pub fn clear<R: Renderer<Image = T>>(&mut self, renderer: &mut R) {
        for (idx, image) in self.0.drain() {
            renderer.delete_image(image.1, ImageId(idx));
        }
    }
}

/// Specifies the type of filter to apply to images with `crate::Canvas::filter_image`.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum ImageFilter {
    /// Applies a Gaussian blur filter with the specified standard deviation.
    GaussianBlur {
        /// The standard deviation of the Gaussian blur filter.
        sigma: f32,
    },
    /// Applies a 4x5 color matrix, the operation behind SVG `feColorMatrix` and
    /// the CSS/Canvas `filter` color functions (`grayscale`, `sepia`, ...).
    ///
    /// The 20 values are row-major: the output channel `[r', g', b', a']` is
    /// `M * [r, g, b, a, 1]`, i.e. `r' = m[0]*r + m[1]*g + m[2]*b + m[3]*a +
    /// m[4]`, and so on for rows `m[5..10]`, `m[10..15]`, `m[15..20]`. The matrix
    /// is applied in **unpremultiplied, sRGB** space (matching the CSS filter
    /// functions, which are defined in sRGB) and the result is clamped to
    /// `[0, 1]`, so overflowing matrices cannot produce out-of-range or NaN
    /// pixels. Use the constructors below for the standard CSS functions.
    ColorMatrix {
        /// Row-major 4x5 color matrix.
        matrix: [f32; 20],
    },
}

impl ImageFilter {
    /// The identity color matrix (leaves an image unchanged).
    pub const IDENTITY_MATRIX: [f32; 20] = [
        1.0, 0.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];

    // sRGB / Rec.709 luma weights used by the CSS `grayscale`/`saturate`/
    // `hue-rotate` functions (Filter Effects Level 1).
    const LR: f32 = 0.2126;
    const LG: f32 = 0.7152;
    const LB: f32 = 0.0722;

    /// CSS `grayscale(amount)`; `amount` is clamped to `[0, 1]` (1 = fully gray).
    pub fn grayscale(amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let inv = 1.0 - a;
        let (lr, lg, lb) = (Self::LR, Self::LG, Self::LB);
        Self::ColorMatrix {
            matrix: [
                lr + 0.7874 * inv,
                lg - lg * inv,
                lb - lb * inv,
                0.0,
                0.0, //
                lr - lr * inv,
                lg + 0.2848 * inv,
                lb - lb * inv,
                0.0,
                0.0, //
                lr - lr * inv,
                lg - lg * inv,
                lb + 0.9278 * inv,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// CSS `sepia(amount)`; `amount` is clamped to `[0, 1]`.
    pub fn sepia(amount: f32) -> Self {
        let inv = 1.0 - amount.clamp(0.0, 1.0);
        Self::ColorMatrix {
            matrix: [
                0.393 + 0.607 * inv,
                0.769 - 0.769 * inv,
                0.189 - 0.189 * inv,
                0.0,
                0.0, //
                0.349 - 0.349 * inv,
                0.686 + 0.314 * inv,
                0.168 - 0.168 * inv,
                0.0,
                0.0, //
                0.272 - 0.272 * inv,
                0.534 - 0.534 * inv,
                0.131 + 0.869 * inv,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// CSS `saturate(amount)` (`feColorMatrix type="saturate"`). `amount` >= 0;
    /// 0 desaturates, 1 is identity, >1 over-saturates. Uses the SVG
    /// 0.213/0.715/0.072 luma weights.
    pub fn saturate(amount: f32) -> Self {
        let s = amount.max(0.0);
        let (lr, lg, lb) = (0.213f32, 0.715f32, 0.072f32);
        Self::ColorMatrix {
            matrix: [
                lr + 0.787 * s,
                lg - lg * s,
                lb - lb * s,
                0.0,
                0.0, //
                lr - lr * s,
                lg + 0.285 * s,
                lb - lb * s,
                0.0,
                0.0, //
                lr - lr * s,
                lg - lg * s,
                lb + 0.928 * s,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// CSS `hue-rotate(radians)` (`feColorMatrix type="hueRotate"`).
    pub fn hue_rotate(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        let (lr, lg, lb) = (0.213f32, 0.715f32, 0.072f32);
        Self::ColorMatrix {
            matrix: [
                lr + c * 0.787 - s * 0.213,
                lg - c * lg - s * lg,
                lb - c * lb + s * 0.928,
                0.0,
                0.0, //
                lr - c * lr + s * 0.143,
                lg + c * 0.285 + s * 0.140,
                lb - c * lb - s * 0.283,
                0.0,
                0.0, //
                lr - c * lr - s * 0.787,
                lg - c * lg + s * lg,
                lb + c * 0.928 + s * lb,
                0.0,
                0.0, //
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ],
        }
    }

    /// CSS `brightness(amount)`; `amount` >= 0 scales each RGB channel.
    pub fn brightness(amount: f32) -> Self {
        let a = amount.max(0.0);
        Self::ColorMatrix {
            matrix: [
                a, 0.0, 0.0, 0.0, 0.0, //
                0.0, a, 0.0, 0.0, 0.0, //
                0.0, 0.0, a, 0.0, 0.0, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        }
    }

    /// CSS `contrast(amount)`; `amount` >= 0. 1 is identity.
    pub fn contrast(amount: f32) -> Self {
        let a = amount.max(0.0);
        let b = 0.5 - 0.5 * a;
        Self::ColorMatrix {
            matrix: [
                a, 0.0, 0.0, 0.0, b, //
                0.0, a, 0.0, 0.0, b, //
                0.0, 0.0, a, 0.0, b, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        }
    }

    /// CSS `invert(amount)`; `amount` is clamped to `[0, 1]`.
    pub fn invert(amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let d = 1.0 - 2.0 * a;
        Self::ColorMatrix {
            matrix: [
                d, 0.0, 0.0, 0.0, a, //
                0.0, d, 0.0, 0.0, a, //
                0.0, 0.0, d, 0.0, a, //
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        }
    }

    /// CSS `opacity(amount)`; `amount` is clamped to `[0, 1]` and scales alpha.
    pub fn opacity(amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        Self::ColorMatrix {
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 0.0, a, 0.0,
            ],
        }
    }

    /// Folds this filter with `next` (applied after it) into a single
    /// equivalent filter when both are color matrices.
    ///
    /// This is the load-bearing rule for filter chains: a run of N adjacent
    /// color operations costs one GPU pass and zero intermediate textures,
    /// because 4x5 matrices compose by multiplication on the CPU. The fold is
    /// exact in unpremultiplied space - the same space the shader applies the
    /// matrix in - matching how Skia folds via `asAColorMatrix`/`Compose`.
    /// Returns `None` when either side is not a color matrix (a blur cannot
    /// fold), leaving chain execution to run them as separate passes.
    pub fn fold_with(&self, next: &ImageFilter) -> Option<ImageFilter> {
        let (ImageFilter::ColorMatrix { matrix: a }, ImageFilter::ColorMatrix { matrix: b }) = (self, next) else {
            return None;
        };
        // `self` runs first, `next` second: out = B * augment(A), where
        // augment(A) extends the 4x5 matrix with the implicit [0 0 0 0 1] row
        // so the constant column composes correctly.
        let mut m = [0.0f32; 20];
        for row in 0..4 {
            for col in 0..5 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += b[row * 5 + k] * a[k * 5 + col];
                }
                if col == 4 {
                    // The implicit augmented row contributes next's constant.
                    sum += b[row * 5 + 4];
                }
                m[row * 5 + col] = sum;
            }
        }
        Some(ImageFilter::ColorMatrix { matrix: m })
    }
}

#[cfg(test)]
mod filter_fold_tests {
    use super::ImageFilter;

    fn apply(m: &[f32; 20], px: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for row in 0..4 {
            out[row] = m[row * 5] * px[0]
                + m[row * 5 + 1] * px[1]
                + m[row * 5 + 2] * px[2]
                + m[row * 5 + 3] * px[3]
                + m[row * 5 + 4];
        }
        out
    }

    fn matrix(f: &ImageFilter) -> [f32; 20] {
        let ImageFilter::ColorMatrix { matrix } = f else {
            panic!("not a color matrix")
        };
        *matrix
    }

    /// The fold must equal sequential application for arbitrary pixels - the
    /// property that lets a chain of N color ops run as one GPU pass.
    #[test]
    fn folding_matches_sequential_application() {
        let first = ImageFilter::sepia(0.8);
        let second = ImageFilter::hue_rotate(1.1);
        let folded = first.fold_with(&second).expect("two color matrices fold");

        for px in [
            [1.0, 0.0, 0.0, 1.0],
            [0.2, 0.7, 0.4, 0.5],
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0, 1.0],
            [0.9, 0.1, 0.6, 0.3],
        ] {
            let sequential = apply(&matrix(&second), apply(&matrix(&first), px));
            let one_pass = apply(&matrix(&folded), px);
            for c in 0..4 {
                assert!(
                    (sequential[c] - one_pass[c]).abs() < 1e-5,
                    "channel {c} of {px:?}: sequential {} vs folded {}",
                    sequential[c],
                    one_pass[c]
                );
            }
        }
    }

    /// Folding with an identity leaves the other matrix unchanged, and the
    /// constant column (brightness offsets, invert) composes in order.
    #[test]
    fn folding_respects_order_and_identity() {
        let invert = ImageFilter::invert(1.0);
        let bright = ImageFilter::brightness(2.0);
        // invert then brighten: 2*(1-c) ; brighten then invert: 1-2c. Distinct.
        let a = matrix(&invert.fold_with(&bright).unwrap());
        let b = matrix(&bright.fold_with(&invert).unwrap());
        let px = [0.25, 0.5, 0.75, 1.0];
        let ab = apply(&a, px);
        let ba = apply(&b, px);
        assert!(
            (ab[0] - 2.0 * (1.0 - 0.25)).abs() < 1e-5,
            "invert-then-brighten got {}",
            ab[0]
        );
        assert!(
            (ba[0] - (1.0 - 2.0 * 0.25)).abs() < 1e-5,
            "brighten-then-invert got {}",
            ba[0]
        );

        let identity = ImageFilter::saturate(1.0);
        let folded = ImageFilter::sepia(1.0).fold_with(&identity).unwrap();
        let direct = matrix(&ImageFilter::sepia(1.0));
        for (x, y) in matrix(&folded).iter().zip(direct.iter()) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    /// Blurs cannot fold - the chain executor must run them as passes.
    #[test]
    fn blur_does_not_fold() {
        let blur = ImageFilter::GaussianBlur { sigma: 2.0 };
        assert!(blur.fold_with(&ImageFilter::sepia(1.0)).is_none());
        assert!(ImageFilter::sepia(1.0).fold_with(&blur).is_none());
    }
}
