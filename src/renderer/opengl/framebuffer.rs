// TODO: Path rendering only needs stencil attachment, try to get rid of the depth attachment

//use crate::PixelFormat;

use super::{gl, gl::types::*, Texture};

pub struct Framebuffer {
    fbo: GLuint,
    depth_stencil_rbo: GLuint,
}

impl Framebuffer {
    pub fn new(texture: &Texture) -> Self {
        let mut fbo = 0;

        unsafe {
            gl::GenFramebuffers(1, &mut fbo);
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
        }

        let width = texture.info().width() as u32;
        let height = texture.info().height() as u32;

        unsafe {
            gl::FramebufferTexture2D(gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, texture.id(), 0);
        }

        let depth_stencil_rbo = Self::gen_depth_stencil_rbo(width, height);

        unsafe {
            gl::FramebufferRenderbuffer(
                gl::FRAMEBUFFER,
                gl::DEPTH_STENCIL_ATTACHMENT,
                gl::RENDERBUFFER,
                depth_stencil_rbo,
            );

            let status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);

            if status != gl::FRAMEBUFFER_COMPLETE {
                // TODO: do not panic here
                match status {
                    gl::FRAMEBUFFER_INCOMPLETE_ATTACHMENT => panic!("({}) Framebuffer incomplete attachment", status),
                    //gl::FRAMEBUFFER_INCOMPLETE_DRAW_BUFFER => panic!("({}) Framebuffer incomplete draw buffer", status),
                    //gl::FRAMEBUFFER_INCOMPLETE_LAYER_TARGETS => panic!("({}) Framebuffer incomplete layer targets", status),
                    gl::FRAMEBUFFER_INCOMPLETE_DIMENSIONS => panic!("({}) Framebuffer incomplete dimensions", status),
                    gl::FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT => {
                        panic!("({}) Framebuffer incomplete missing attachment", status)
                    }
                    gl::FRAMEBUFFER_INCOMPLETE_MULTISAMPLE => panic!("({}) Framebuffer incomplete multisample", status),
                    //gl::FRAMEBUFFER_INCOMPLETE_READ_BUFFER => panic!("({}) Framebuffer incomplete read buffer", status),
                    gl::FRAMEBUFFER_UNSUPPORTED => panic!("({}) Framebuffer unsupported", status),
                    _ => panic!("({}) Framebuffer not complete!", status),
                };
            }

            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }

        Framebuffer { fbo, depth_stencil_rbo }
    }

    pub fn bind(&self) {
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo);
        }
    }

    pub fn unbind() {
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
        }
    }

    // pub fn blit_to_texture(&self, texture: &Texture) {
    //     let dest_fbo = Self::new(texture);

    //     unsafe {
    //         gl::BindFramebuffer(gl::READ_FRAMEBUFFER, self.fbo);
    //         gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, dest_fbo.fbo);

    //         gl::BlitFramebuffer(
    //             0,
    //             0,
    //             self.width as i32,
    //             self.height as i32,
    //             0,
    //             0,
    //             dest_fbo.width as i32,
    //             dest_fbo.height as i32,
    //             gl::COLOR_BUFFER_BIT,
    //             gl::NEAREST
    //         );

    //         gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
    //         gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
    //     }
    // }

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
        }
    }
}
