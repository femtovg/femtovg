
use std::ptr;
use std::ffi::{CStr, CString};

use crate::{
    Result,
    ErrorKind
};

use super::gl;
use super::gl::types::*;

pub(crate) struct Shader {
    id: GLuint
}

impl Shader {
    pub fn new(src: &CStr, kind: GLenum) -> Result<Self> {
        let id = unsafe { gl::CreateShader(kind) };

        // Compile
        unsafe {
            gl::ShaderSource(id, 1, &src.as_ptr(), ptr::null());
            gl::CompileShader(id);
        }

        // Validate
        let mut success: GLint = 1;
        unsafe { gl::GetShaderiv(id, gl::COMPILE_STATUS, &mut success); }

        if success == 0 {
            let mut len = 0;
            unsafe { gl::GetShaderiv(id, gl::INFO_LOG_LENGTH, &mut len); }

            let error = create_whitespace_cstring_with_len(len as usize);

            unsafe {
                gl::GetShaderInfoLog(id, len, ptr::null_mut(), error.as_ptr() as *mut GLchar);
            }

            let name = match kind {
                gl::VERTEX_SHADER => "Vertex stage",
                gl::FRAGMENT_SHADER => "Fragment stage",
                _ => "Shader stage"
            };

            return Err(ErrorKind::ShaderCompileError(format!("{}: {}", name, error.to_string_lossy())));
        }

        Ok(Shader {
            id
        })
    }

    pub fn id(&self) -> GLuint {
        self.id
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteShader(self.id);
        }
    }
}

pub(crate) struct Program {
    id: GLuint,
    loc_viewsize: GLint,
    loc_tex: GLint,
    loc_masktex: GLint,
    loc_frag: GLint,
}

impl Program {

    pub fn new(shaders: &[Shader]) -> Result<Self> {
        let mut program = Self {
            id: unsafe { gl::CreateProgram() },
            loc_viewsize: Default::default(),
            loc_tex: Default::default(),
            loc_masktex: Default::default(),
            loc_frag: Default::default(),
        };

        // Attach stages
        for shader in shaders {
            unsafe { gl::AttachShader(program.id, shader.id()); }
        }

        unsafe {
            gl::BindAttribLocation(program.id, 0, CString::new("vertex")?.as_ptr());
            gl::BindAttribLocation(program.id, 1, CString::new("tcoord")?.as_ptr());

            gl::LinkProgram(program.id);
        }

        // Check for error
        let mut success: GLint = 1;
        unsafe { gl::GetProgramiv(program.id, gl::LINK_STATUS, &mut success); }

        if success == 0 {
            let mut len: GLint = 0;
            unsafe { gl::GetProgramiv(program.id, gl::INFO_LOG_LENGTH, &mut len); }

            let error = create_whitespace_cstring_with_len(len as usize);

            unsafe {
                gl::GetProgramInfoLog(program.id, len, ptr::null_mut(), error.as_ptr() as *mut GLchar);
            }

            return Err(ErrorKind::ShaderLinkError(error.to_string_lossy().into_owned()));
        }

        // Detach stages
        for shader in shaders {
            unsafe { gl::DetachShader(program.id, shader.id()); }
        }

        // Uniform locations
        unsafe {
            program.loc_viewsize = gl::GetUniformLocation(program.id, CString::new("viewSize")?.as_ptr());
            program.loc_tex = gl::GetUniformLocation(program.id, CString::new("tex")?.as_ptr());
            program.loc_masktex = gl::GetUniformLocation(program.id, CString::new("masktex")?.as_ptr());
            program.loc_frag = gl::GetUniformLocation(program.id, CString::new("frag")?.as_ptr());
        }

        Ok(program)
    }

    pub(crate) fn bind(&self) {
        unsafe { gl::UseProgram(self.id); }
    }

    pub(crate) fn unbind(&self) {
        unsafe { gl::UseProgram(0); }
    }

    pub(crate) fn set_tex(&self, tex: GLint) {
        unsafe { gl::Uniform1i(self.loc_tex, tex); }
    }

    pub(crate) fn set_masktex(&self, tex: GLint) {
        unsafe { gl::Uniform1i(self.loc_masktex, tex); }
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

impl Drop for Program {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.id);
        }
    }
}

// CString buffer for GetShaderInfoLog and GetProgramInfoLog
fn create_whitespace_cstring_with_len(len: usize) -> CString {
    let mut buffer: Vec<u8> = Vec::with_capacity(len + 1);
    buffer.extend([b' '].iter().cycle().take(len));
    unsafe { CString::from_vec_unchecked(buffer) }
}
