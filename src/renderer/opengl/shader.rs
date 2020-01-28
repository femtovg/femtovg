
use std::str;
use std::ptr;
use std::{error::Error, fmt};
use std::ffi::{NulError, CString};

use super::gl;
use super::gl::types::*;

pub(crate) struct Shader {
    prog: GLuint,
    vert: GLuint,
    frag: GLuint,
    loc_viewsize: GLint,
    loc_tex: GLint,
    loc_frag: GLint,
}

impl Shader {

    pub fn new(opts: &str, vertex_src: &str, fragment_src: &str) -> Result<Self, ShaderError> {

        let vertex_src = CString::new(format!("#version 100\n{}\n{}", opts, vertex_src))?;
        let fragment_src = CString::new(format!("#version 100\n{}\n{}", opts, fragment_src))?;

        let mut shader = unsafe {
            Self {
                prog: gl::CreateProgram(),
                vert: gl::CreateShader(gl::VERTEX_SHADER),
                frag: gl::CreateShader(gl::FRAGMENT_SHADER),
                loc_viewsize: Default::default(),
                loc_tex: Default::default(),
                loc_frag: Default::default(),
            }
        };

        // Compile and link
        unsafe {
            gl::ShaderSource(shader.vert, 1, &vertex_src.as_ptr(), ptr::null());
            gl::CompileShader(shader.vert);
            Self::check_shader_ok(shader.vert, "vertex")?;

            gl::ShaderSource(shader.frag, 1, &fragment_src.as_ptr(), ptr::null());
            gl::CompileShader(shader.frag);
            Self::check_shader_ok(shader.frag, "fragment")?;

            gl::AttachShader(shader.prog, shader.vert);
            gl::AttachShader(shader.prog, shader.frag);

            gl::BindAttribLocation(shader.prog, 0, CString::new("vertex")?.as_ptr());
            gl::BindAttribLocation(shader.prog, 1, CString::new("tcoord")?.as_ptr());

            gl::LinkProgram(shader.prog);

            let mut success = i32::from(gl::FALSE);
            gl::GetProgramiv(shader.prog, gl::LINK_STATUS, &mut success);

            if success != i32::from(gl::TRUE) {
                let mut log_length = 0;
                gl::GetProgramiv(shader.prog, gl::INFO_LOG_LENGTH, &mut log_length);

                let mut info_log = Vec::with_capacity(log_length as usize);
                info_log.set_len((log_length as usize) - 1);

                gl::GetProgramInfoLog(shader.prog, log_length, ptr::null_mut(), info_log.as_mut_ptr() as *mut GLchar);

                return Err(match str::from_utf8(&info_log) {
                    Ok(msg) => ShaderError::ProgramLinkError(msg.to_string()),
                    Err(err) => ShaderError::ProgramLinkError(format!("{}", err)),
                });
            }
        }

        // Uniform locations
        unsafe {
            shader.loc_viewsize = gl::GetUniformLocation(shader.prog, CString::new("viewSize")?.as_ptr());
            shader.loc_tex = gl::GetUniformLocation(shader.prog, CString::new("tex")?.as_ptr());
            shader.loc_frag = gl::GetUniformLocation(shader.prog, CString::new("frag")?.as_ptr());
        }

        Ok(shader)
    }

    fn check_shader_ok(shader: GLuint, stage: &str) -> Result<(), ShaderError> {
        let mut success = i32::from(gl::FALSE);

        unsafe { gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success); }

        if success != i32::from(gl::TRUE) {
            let mut log_length = 0;

            let info_log = unsafe {
                gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut log_length);

                let mut info_log = Vec::with_capacity(log_length as usize);
                info_log.set_len((log_length as usize) - 1);

                gl::GetShaderInfoLog(shader, log_length, ptr::null_mut(), info_log.as_mut_ptr() as *mut GLchar);

                info_log
            };

            Err(match str::from_utf8(&info_log) {
                Ok(msg) => ShaderError::CompileError(format!("{} {}", stage, msg)),
                Err(err) => ShaderError::CompileError(format!("{} {}", stage, err)),
            })
        } else {
            Ok(())
        }
    }

    pub(crate) fn bind(&self) {
        unsafe { gl::UseProgram(self.prog); }
    }

    pub(crate) fn unbind(&self) {
        unsafe { gl::UseProgram(0); }
    }

    pub(crate) fn set_tex(&self, tex: GLint) {
        unsafe { gl::Uniform1i(self.loc_tex, tex); }
    }

    pub(crate) fn set_view(&self, view: [f32; 2]) {
        unsafe { gl::Uniform2fv(self.loc_viewsize, 1, view.as_ptr()); }
    }

    pub(crate) fn set_config(&self, count: i32, ptr: *const f32) {
        unsafe {
            gl::Uniform4fv(self.loc_frag, count, ptr);
        }
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.prog);
            gl::DeleteShader(self.frag);
            gl::DeleteShader(self.vert);
        }
    }
}

#[derive(Debug)]
pub enum ShaderError {
    CompileError(String),
    ProgramLinkError(String),
    GeneralError(String)
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid first item to double")
    }
}

impl From<NulError> for ShaderError {
    fn from(error: NulError) -> Self {
        ShaderError::GeneralError(error.description().to_string())
    }
}

impl Error for ShaderError {}
