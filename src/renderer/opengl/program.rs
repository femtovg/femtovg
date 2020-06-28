
use std::ptr;
use std::ffi::{CStr, CString};

use crate::ErrorKind;

use super::gl;
use super::gl::types::*;

const GLSL_VERSION: &str = "#version 100";

pub(crate) struct Shader {
    id: GLuint
}

impl Shader {
    pub fn new(src: &CStr, kind: GLenum) -> Result<Self, ErrorKind> {
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
}

impl Program {

    pub fn new(shaders: &[Shader], attrib_locations: &[&str]) -> Result<Self, ErrorKind> {
        let program = Self {
            id: unsafe { gl::CreateProgram() },
        };
        
        // Attach stages
        for shader in shaders {
            unsafe { gl::AttachShader(program.id, shader.id()); }
        }
        
        for (i, loc) in attrib_locations.iter().enumerate() {
			unsafe {
				gl::BindAttribLocation(program.id, i as u32, CString::new(*loc)?.as_ptr());
			}
		}

        unsafe {
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
        
        Ok(program)
    }

    pub(crate) fn bind(&self) {
        unsafe { gl::UseProgram(self.id); }
    }

    pub(crate) fn unbind(&self) {
        unsafe { gl::UseProgram(0); }
    }

    fn uniform_location(&self, name: &str) -> Result<GLint, ErrorKind> {
        unsafe {
            Ok(gl::GetUniformLocation(self.id, CString::new(name)?.as_ptr()))
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

pub struct MainProgram {
    program: Program,
    loc_viewsize: GLint,
    loc_tex: GLint,
    loc_masktex: GLint,
    loc_frag: GLint,
}

impl MainProgram {
    pub(crate) fn new(antialias: bool) -> Result<Self, ErrorKind> {
        let shader_defs = if antialias { "#define EDGE_AA 1" } else { "" };
        let vert_shader_src = format!("{}\n{}\n{}", GLSL_VERSION, shader_defs, include_str!("main-vs.glsl"));
        let frag_shader_src = format!("{}\n{}\n{}", GLSL_VERSION, shader_defs, include_str!("main-fs.glsl"));

        let vert_shader = Shader::new(&CString::new(vert_shader_src)?, gl::VERTEX_SHADER)?;
        let frag_shader = Shader::new(&CString::new(frag_shader_src)?, gl::FRAGMENT_SHADER)?;

        let program = Program::new(
			&[vert_shader, frag_shader],
			&["vertex", "tcoord"]
		)?;

        let loc_viewsize = program.uniform_location("viewSize")?;
        let loc_tex = program.uniform_location("tex")?;
        let loc_masktex = program.uniform_location("masktex")?;
        let loc_frag = program.uniform_location("frag")?;

        Ok(Self {
            program,
            loc_viewsize,
            loc_tex,
            loc_masktex,
            loc_frag,
        })
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

    pub(crate) fn bind(&self) {
        self.program.bind();
    }

    pub(crate) fn unbind(&self) {
        self.program.unbind();
    }
}

pub struct BlurProgram {
    program: Program,
    loc_image: GLint,
    loc_horizontal: GLint,
    loc_image_size: GLint,
}

impl BlurProgram {
    pub fn new() -> Result<Self, ErrorKind> {
        let vert_shader_src = format!("{}\n{}", GLSL_VERSION, include_str!("blur-vs.glsl"));
        let frag_shader_src = format!("{}\n{}", GLSL_VERSION, include_str!("blur-fs.glsl"));

        let vert_shader = Shader::new(&CString::new(vert_shader_src)?, gl::VERTEX_SHADER)?;
        let frag_shader = Shader::new(&CString::new(frag_shader_src)?, gl::FRAGMENT_SHADER)?;

        let program = Program::new(
			&[vert_shader, frag_shader],
			&["vertex", "tcoord"]
		)?;

        let loc_image = program.uniform_location("image")?;
        let loc_horizontal = program.uniform_location("horizontal")?;
        let loc_image_size = program.uniform_location("image_size")?;

        Ok(Self {
            program,
            loc_image,
            loc_horizontal,
            loc_image_size
        })
    }

    pub(crate) fn set_image(&self, image: GLint) {
        unsafe { gl::Uniform1i(self.loc_image, image); }
    }

    pub(crate) fn set_horizontal(&self, horizontal: bool) {
        unsafe { gl::Uniform1i(self.loc_horizontal, horizontal as i32); }
    }

    pub(crate) fn set_image_size(&self, size: [f32; 2]) {
        unsafe { gl::Uniform2fv(self.loc_image_size, 1, size.as_ptr()); }
    }

    pub(crate) fn bind(&self) {
        self.program.bind();
    }

    pub(crate) fn unbind(&self) {
        self.program.unbind();
    }
}

// CString buffer for GetShaderInfoLog and GetProgramInfoLog
fn create_whitespace_cstring_with_len(len: usize) -> CString {
    let mut buffer: Vec<u8> = Vec::with_capacity(len + 1);
    buffer.extend([b' '].iter().cycle().take(len));
    unsafe { CString::from_vec_unchecked(buffer) }
}
