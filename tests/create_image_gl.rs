//! `Canvas::create_image` on the OpenGL backend: a source with packed rows is uploaded once, as the
//! texture is created, and a strided one through `glTexSubImage2D`; either way the texture holds
//! exactly the source pixels, for each format, an odd width (rows not 4-byte aligned) and with
//! mipmaps. Runs on desktop GL and on the renderer's GLES 2.0 branch, on a headless EGL device
//! (Mesa's llvmpipe works); without one it skips, unless `FEMTOVG_REQUIRE_GPU` is `gl`. The GLES 2.0
//! run covers the renderer's code for that branch, not an ES 2 driver: it reports an ES 3 context
//! as 2.0.
#![cfg(target_os = "linux")]

use std::{cell::Cell, ffi::c_void};

use femtovg::{renderer::OpenGl, Canvas, ImageFlags, ImageId, ImageSource};
use glow::HasContext;
use glutin::{
    api::egl::{context::PossiblyCurrentContext, device::Device, display::Display},
    config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
    context::{ContextApi, ContextAttributesBuilder, Version},
    display::GlDisplay,
};
use imgref::{Img, ImgRef};
use rgb::{alt::Gray, RGB8, RGBA8};

thread_local! {
    /// The driver's `glTexSubImage2D`, which the counting hook calls through to.
    static TEX_SUB_IMAGE: Cell<usize> = const { Cell::new(0) };
    /// The driver's `glGetString`, which the version hook calls through to.
    static GET_STRING: Cell<usize> = const { Cell::new(0) };
    /// The `glTexSubImage2D` calls made on this thread.
    static SUB_UPLOADS: Cell<usize> = const { Cell::new(0) };
}

type TexSubImage2d = extern "system" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);

extern "system" fn counting_tex_sub_image(
    target: u32,
    level: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    format: u32,
    ty: u32,
    pixels: *const c_void,
) {
    SUB_UPLOADS.set(SUB_UPLOADS.get() + 1);
    let real: TexSubImage2d = unsafe { std::mem::transmute(TEX_SUB_IMAGE.get()) };
    real(target, level, x, y, width, height, format, ty, pixels)
}

/// Reports the ES 3 context Mesa gives for a 2.0 request as 2.0, so the renderer takes its
/// GLES 2.0 path (LUMINANCE, no `UNPACK_ROW_LENGTH`).
extern "system" fn es2_get_string(name: u32) -> *const u8 {
    if name == glow::VERSION {
        return c"OpenGL ES 2.0".as_ptr().cast();
    }
    let real: extern "system" fn(u32) -> *const u8 = unsafe { std::mem::transmute(GET_STRING.get()) };
    real(name)
}

struct Gl {
    canvas: Canvas<OpenGl>,
    gl: glow::Context,
    es2: bool,
    _context: PossiblyCurrentContext,
}

fn headless_gl(es2: bool) -> Option<Gl> {
    let api = if es2 {
        ContextApi::Gles(Some(Version::new(2, 0)))
    } else {
        ContextApi::OpenGl(None)
    };
    let found = Device::query_devices().ok().and_then(|mut devices| {
        devices.find_map(|device| unsafe {
            let display = Display::with_device(&device, None).ok()?;
            let template = ConfigTemplateBuilder::new()
                .with_surface_type(ConfigSurfaceTypes::empty())
                .build();
            let config = display.find_configs(template).ok()?.next()?;
            let context = display
                .create_context(
                    &config,
                    &ContextAttributesBuilder::new().with_context_api(api).build(None),
                )
                .ok()?;
            Some((display, context.make_current_surfaceless().ok()?))
        })
    });
    let Some((display, context)) = found else {
        let require = std::env::var("FEMTOVG_REQUIRE_GPU").unwrap_or_default();
        assert!(
            !require.trim().eq_ignore_ascii_case("gl"),
            "FEMTOVG_REQUIRE_GPU=gl but no EGL device was found"
        );
        eprintln!("skipping: no EGL device");
        return None;
    };
    let gl = unsafe { glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name)) };
    let renderer = unsafe {
        OpenGl::new_from_function_cstr(|name| {
            let real = display.get_proc_address(name);
            match name.to_bytes() {
                b"glTexSubImage2D" => {
                    TEX_SUB_IMAGE.set(real as usize);
                    counting_tex_sub_image as *const c_void
                }
                b"glGetString" if es2 => {
                    GET_STRING.set(real as usize);
                    es2_get_string as *const c_void
                }
                _ => real,
            }
        })
    }
    .expect("renderer");
    eprintln!(
        "GL: {} {}",
        unsafe { gl.get_parameter_string(glow::RENDERER) },
        unsafe { gl.get_parameter_string(glow::VERSION) }
    );
    Some(Gl {
        canvas: Canvas::new(renderer).expect("canvas"),
        gl,
        es2,
        _context: context,
    })
}

impl Gl {
    /// Creates an image and checks the `glTexSubImage2D` calls it took: none when the texture was
    /// created with the pixels, one when it was created empty and then filled.
    fn create<'a>(&mut self, src: impl Into<ImageSource<'a>>, flags: ImageFlags, sub_uploads: usize) -> ImageId {
        SUB_UPLOADS.set(0);
        let id = self.canvas.create_image(src, flags).expect("create_image");
        assert_eq!(SUB_UPLOADS.get(), sub_uploads, "glTexSubImage2D calls");
        assert_eq!(unsafe { self.gl.get_error() }, glow::NO_ERROR);
        id
    }

    /// Level 0 of an image's texture, read back as RGBA.
    fn read(&self, id: ImageId, width: usize, height: usize) -> Vec<[u8; 4]> {
        let texture = self.canvas.get_native_texture(id).expect("texture");
        let mut out = vec![0u8; width * height * 4];
        unsafe {
            let fbo = self.gl.create_framebuffer().expect("framebuffer");
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            assert_eq!(
                self.gl.check_framebuffer_status(glow::FRAMEBUFFER),
                glow::FRAMEBUFFER_COMPLETE
            );
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut out)),
            );
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.delete_framebuffer(fbo);
        }
        out.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect()
    }

    /// Creates `src` packed and as a strided view into a wider, taller buffer, and checks the path
    /// and the pixels of each. `rgba` gives a pixel as read back, or `None` when the texture
    /// can't be read (LUMINANCE is not color-renderable).
    fn check<T: Copy + Default>(
        &mut self,
        name: &str,
        src: ImgRef<T>,
        flags: ImageFlags,
        rgba: Option<fn(T) -> [u8; 4]>,
    ) where
        for<'a> ImgRef<'a, T>: Into<ImageSource<'a>>,
    {
        let (w, h) = (src.width(), src.height());
        let expected = rgba.map(|rgba| src.pixels().map(rgba).collect::<Vec<_>>());

        let id = self.create(src, flags, 0);
        if let Some(expected) = &expected {
            assert_eq!(&self.read(id, w, h), expected, "{name} {w}x{h}, es2: {}", self.es2);
        }

        let mut wide = Img::new(vec![T::default(); (w + 3) * (h + 2)], w + 3, h + 2);
        for (row, src_row) in wide.sub_image_mut(1, 1, w, h).rows_mut().zip(src.rows()) {
            row.copy_from_slice(src_row);
        }
        let id = self.create(wide.sub_image(1, 1, w, h), flags, 1);
        if let Some(expected) = &expected {
            assert_eq!(
                &self.read(id, w, h),
                expected,
                "strided {name} {w}x{h}, es2: {}",
                self.es2
            );
        }
    }
}

fn check_formats(es2: bool) {
    let Some(mut gl) = headless_gl(es2) else { return };
    let (w, h) = (3, 2);

    let rgb: Vec<RGB8> = (0..w * h)
        .map(|i| RGB8::new(10 * i as u8, 100 + i as u8, 200 - i as u8))
        .collect();
    gl.check(
        "RGB",
        Img::new(&rgb[..], w, h),
        ImageFlags::empty(),
        Some(|p| [p.r, p.g, p.b, 255]),
    );

    let gray: Vec<Gray<u8>> = (0..w * h).map(|i| Gray::new(30 * i as u8 + 1)).collect();
    let gray_rgba: fn(Gray<u8>) -> [u8; 4] = |p| [p.value(), 0, 0, 255];
    gl.check(
        "Gray8",
        Img::new(&gray[..], w, h),
        ImageFlags::empty(),
        (!es2).then_some(gray_rgba),
    );

    let rgba_pixels = |w: usize, h: usize| -> Vec<RGBA8> {
        (0..w * h)
            .map(|i| RGBA8::new(i as u8, 50, 90 + i as u8, 255 - i as u8))
            .collect()
    };
    let rgba_rgba: fn(RGBA8) -> [u8; 4] = |p| [p.r, p.g, p.b, p.a];
    gl.check(
        "RGBA",
        Img::new(&rgba_pixels(w, h)[..], w, h),
        ImageFlags::empty(),
        Some(rgba_rgba),
    );
    // GLES 2.0 generates mipmaps only for power-of-two sizes.
    let (w, h) = if es2 { (4, 4) } else { (w, h) };
    gl.check(
        "RGBA with mipmaps",
        Img::new(&rgba_pixels(w, h)[..], w, h),
        ImageFlags::GENERATE_MIPMAPS,
        Some(rgba_rgba),
    );
}

#[test]
fn created_images_hold_their_pixels() {
    check_formats(false);
}

#[test]
fn created_images_hold_their_pixels_gles2() {
    check_formats(true);
}
