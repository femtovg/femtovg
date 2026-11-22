//! `Canvas::create_image` on the OpenGL backend: the texture holds exactly the source
//! pixels, for each format, an odd width (rows not 4-byte aligned) and with mipmaps.
//! Runs on a headless EGL device (Mesa's llvmpipe works); without one it skips, unless
//! `FEMTOVG_REQUIRE_GPU` is `gl` or another non-empty value.
#![cfg(target_os = "linux")]

use std::ffi::CStr;

use femtovg::{renderer::OpenGl, Canvas, ImageFlags};
use glow::HasContext;
use glutin::{
    api::egl::{context::PossiblyCurrentContext, device::Device, display::Display},
    config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
    context::ContextAttributesBuilder,
    display::GlDisplay,
};
use imgref::Img;
use rgb::{alt::Gray, RGB8, RGBA8};

struct Gl {
    canvas: Canvas<OpenGl>,
    gl: glow::Context,
    _context: PossiblyCurrentContext,
}

fn headless_gl() -> Option<Gl> {
    let found = Device::query_devices().ok().and_then(|mut devices| {
        devices.find_map(|device| unsafe {
            let display = Display::with_device(&device, None).ok()?;
            let template = ConfigTemplateBuilder::new()
                .with_surface_type(ConfigSurfaceTypes::empty())
                .build();
            let config = display.find_configs(template).ok()?.next()?;
            let context = display
                .create_context(&config, &ContextAttributesBuilder::new().build(None))
                .ok()?;
            Some((display, context.make_current_surfaceless().ok()?))
        })
    });
    let Some((display, context)) = found else {
        let require = std::env::var("FEMTOVG_REQUIRE_GPU").unwrap_or_default();
        assert!(
            require.trim().is_empty(),
            "FEMTOVG_REQUIRE_GPU is set but no EGL device was found"
        );
        eprintln!("skipping: no EGL device");
        return None;
    };
    let load = |name: &CStr| display.get_proc_address(name);
    let gl = unsafe { glow::Context::from_loader_function_cstr(load) };
    eprintln!("GL: {}", unsafe { gl.get_parameter_string(glow::RENDERER) });
    let canvas = Canvas::new(unsafe { OpenGl::new_from_function_cstr(load) }.expect("renderer")).expect("canvas");
    Some(Gl {
        canvas,
        gl,
        _context: context,
    })
}

impl Gl {
    /// Level 0 of an image's texture, read back as RGBA.
    fn read(&self, id: femtovg::ImageId, width: usize, height: usize) -> Vec<[u8; 4]> {
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
}

#[test]
fn created_images_hold_their_pixels() {
    let Some(mut gl) = headless_gl() else { return };
    let (w, h) = (3, 2);

    let rgb: Vec<RGB8> = (0..w * h)
        .map(|i| RGB8::new(10 * i as u8, 100 + i as u8, 200 - i as u8))
        .collect();
    let id = gl
        .canvas
        .create_image(Img::new(rgb.as_slice(), w, h), ImageFlags::empty())
        .expect("rgb");
    let expected: Vec<_> = rgb.iter().map(|p| [p.r, p.g, p.b, 255]).collect();
    assert_eq!(gl.read(id, w, h), expected, "RGB {w}x{h}");

    let gray: Vec<Gray<u8>> = (0..w * h).map(|i| Gray::new(30 * i as u8 + 1)).collect();
    let id = gl
        .canvas
        .create_image(Img::new(gray.as_slice(), w, h), ImageFlags::empty())
        .expect("gray");
    let expected: Vec<_> = gray.iter().map(|p| [p.value(), 0, 0, 255]).collect();
    assert_eq!(gl.read(id, w, h), expected, "Gray8 {w}x{h}");

    let rgba: Vec<RGBA8> = (0..w * h)
        .map(|i| RGBA8::new(i as u8, 50, 90 + i as u8, 255 - i as u8))
        .collect();
    let id = gl
        .canvas
        .create_image(Img::new(rgba.as_slice(), w, h), ImageFlags::GENERATE_MIPMAPS)
        .expect("rgba");
    let expected: Vec<_> = rgba.iter().map(|p| [p.r, p.g, p.b, p.a]).collect();
    assert_eq!(gl.read(id, w, h), expected, "RGBA {w}x{h} with mipmaps");
}
