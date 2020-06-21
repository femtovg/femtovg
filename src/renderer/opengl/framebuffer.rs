
// TODO: Path rendering only needs stencil attachment, try to get rid of the depth attachment

use crate::PixelFormat;

use super::{
    gl,
    gl::types::*,
    Texture
};

enum FramebufferData {
    Multisampled {
        color_rbo: GLuint,
    },
    Texture {

    }
}

pub struct Framebuffer {
    fbo: GLuint,
    depth_stencil_rbo: GLuint,
    width: u32,
    height: u32,
    data: FramebufferData
}

impl Framebuffer {

    pub fn new_msaa(width: u32, height: u32, format: PixelFormat, samples: u8) -> Self {
        let mut fbo = 0;

        unsafe {
            gl::GenFramebuffers(1, &mut fbo);
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
        }

        let mut color_rbo = 0;

        let internal_format = match format {
            PixelFormat::Gray8 => gl::R8,
            PixelFormat::Rgb8 => gl::RGB,
            PixelFormat::Rgba8 => gl::RGBA,
        };

        unsafe {
            gl::GenRenderbuffers(1, &mut color_rbo);
            gl::BindRenderbuffer(gl::RENDERBUFFER, color_rbo);
            gl::RenderbufferStorageMultisample(gl::RENDERBUFFER, samples as i32, internal_format, width as i32, height as i32);
            gl::BindRenderbuffer(gl::RENDERBUFFER, 0);

            gl::FramebufferRenderbuffer(
                gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::RENDERBUFFER, color_rbo
            );
        }

        let depth_stencil_rbo = Self::gen_depth_stencil_rbo(width, height);

        unsafe {
            gl::FramebufferRenderbuffer(
                gl::FRAMEBUFFER, gl::DEPTH_STENCIL_ATTACHMENT, gl::RENDERBUFFER, depth_stencil_rbo
            );

            if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                panic!("Framebuffer not complete!");
            }
    
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }

        Framebuffer {
            fbo,
            depth_stencil_rbo,
            width,
            height,
            data: FramebufferData::Multisampled {
                color_rbo
            }
        }
    }

    pub fn new_tex(texture: &Texture) -> Self {
        let mut fbo = 0;

        unsafe {
            gl::GenFramebuffers(1, &mut fbo);
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
        }

        let width = texture.info().width() as u32;
        let height = texture.info().height() as u32;

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, texture.id());
            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, texture.id(), 0
            );
        }

        let depth_stencil_rbo = Self::gen_depth_stencil_rbo(width, height);

        unsafe {
            gl::FramebufferRenderbuffer(
                gl::FRAMEBUFFER, gl::DEPTH_STENCIL_ATTACHMENT, gl::RENDERBUFFER, depth_stencil_rbo
            );

            if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                panic!("Framebuffer not complete!");
            }
            
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        Framebuffer {
            fbo,
            depth_stencil_rbo,
            width,
            height,
            data: FramebufferData::Texture {
                
            }
        }
    }

    pub fn bind(&self) {
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo);
        }
    }

    pub fn unbind() {
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }
    }

    pub fn blit_to_texture(&self, texture: &Texture) {
        let dest_fbo = Self::new_tex(texture);

        unsafe {
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, self.fbo);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, dest_fbo.fbo);

            gl::BlitFramebuffer(
                0, 
                0, 
                self.width as i32, 
                self.height as i32, 
                0, 
                0, 
                dest_fbo.width as i32, 
                dest_fbo.height as i32, 
                gl::COLOR_BUFFER_BIT, 
                gl::NEAREST
            );

            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
        }
    }

    fn gen_depth_stencil_rbo(width: u32, height: u32) -> GLuint {
        let mut id = 0;

        unsafe {
            gl::GenRenderbuffers(1, &mut id);
            gl::BindRenderbuffer(gl::RENDERBUFFER, id);
            gl::RenderbufferStorage(gl::RENDERBUFFER, gl::DEPTH24_STENCIL8, width as i32, height as i32);
            gl::BindRenderbuffer(gl::RENDERBUFFER, 0);
        }

        id
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteFramebuffers(1, &self.fbo);
            gl::DeleteRenderbuffers(1, &self.depth_stencil_rbo);

            match self.data {
                FramebufferData::Multisampled { color_rbo } => {
                    gl::DeleteRenderbuffers(1, &color_rbo);
                }
                FramebufferData::Texture {} => {

                }
            }
        }
    }
}