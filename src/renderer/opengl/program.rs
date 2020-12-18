use std::rc::Rc;

use crate::ErrorKind;

use glow::HasContext;

const GLSL_VERSION: &str = "#version 100";

pub(crate) struct Shader {
    context: Rc<glow::Context>,
    id: <glow::Context as glow::HasContext>::Shader,
}

impl Shader {
    pub fn new(context: &Rc<glow::Context>, src: &str, kind: u32) -> Result<Self, ErrorKind> {
        let id = unsafe { context.create_shader(kind).unwrap() };

        // Compile
        unsafe {
            context.shader_source(id, src);
            context.compile_shader(id);
        }

        // Validate

        let success = unsafe { context.get_shader_compile_status(id) };
        if !success {
            let error = unsafe { context.get_shader_info_log(id) };

            let name = match kind {
                glow::VERTEX_SHADER => "Vertex stage",
                glow::FRAGMENT_SHADER => "Fragment stage",
                _ => "Shader stage",
            };

            return Err(ErrorKind::ShaderCompileError(format!("{}: {}", name, error)));
        }

        Ok(Shader {
            context: context.clone(),
            id,
        })
    }

    pub fn id(&self) -> <glow::Context as glow::HasContext>::Shader {
        self.id
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        unsafe {
            self.context.delete_shader(self.id);
        }
    }
}

pub(crate) struct Program {
    context: Rc<glow::Context>,
    id: <glow::Context as glow::HasContext>::Program,
}

impl Program {
    pub fn new(context: &Rc<glow::Context>, shaders: &[Shader], attrib_locations: &[&str]) -> Result<Self, ErrorKind> {
        let program = Self {
            context: context.clone(),
            id: unsafe { context.create_program().unwrap() },
        };

        // Attach stages
        for shader in shaders {
            unsafe {
                context.attach_shader(program.id, shader.id());
            }
        }

        for (i, loc) in attrib_locations.iter().enumerate() {
            unsafe {
                context.bind_attrib_location(program.id, i as u32, *loc);
            }
        }

        unsafe {
            context.link_program(program.id);
        }

        // Check for error

        let success = unsafe { context.get_program_link_status(program.id) };

        if !success {
            let error = unsafe { context.get_program_info_log(program.id) };

            return Err(ErrorKind::ShaderLinkError(error));
        }

        // Detach stages
        for shader in shaders {
            unsafe {
                context.detach_shader(program.id, shader.id());
            }
        }

        Ok(program)
    }

    pub(crate) fn bind(&self) {
        unsafe {
            self.context.use_program(Some(self.id));
        }
    }

    pub(crate) fn unbind(&self) {
        unsafe {
            self.context.use_program(None);
        }
    }

    fn uniform_location(&self, name: &str) -> Result<<glow::Context as glow::HasContext>::UniformLocation, ErrorKind> {
        unsafe { Ok(self.context.get_uniform_location(self.id, name).unwrap()) }
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        unsafe {
            self.context.delete_program(self.id);
        }
    }
}

pub struct MainProgram {
    context: Rc<glow::Context>,
    program: Program,
    loc_viewsize: <glow::Context as glow::HasContext>::UniformLocation,
    loc_tex: <glow::Context as glow::HasContext>::UniformLocation,
    loc_masktex: <glow::Context as glow::HasContext>::UniformLocation,
    loc_frag: <glow::Context as glow::HasContext>::UniformLocation,
}

impl MainProgram {
    pub(crate) fn new(context: &Rc<glow::Context>, antialias: bool) -> Result<Self, ErrorKind> {
        let shader_defs = if antialias { "#define EDGE_AA 1" } else { "" };
        let vert_shader_src = format!("{}\n{}\n{}", GLSL_VERSION, shader_defs, include_str!("main-vs.glsl"));
        let frag_shader_src = format!("{}\n{}\n{}", GLSL_VERSION, shader_defs, include_str!("main-fs.glsl"));

        let vert_shader = Shader::new(context, &vert_shader_src, glow::VERTEX_SHADER)?;
        let frag_shader = Shader::new(context, &frag_shader_src, glow::FRAGMENT_SHADER)?;

        let program = Program::new(context, &[vert_shader, frag_shader], &["vertex", "tcoord"])?;

        let loc_viewsize = program.uniform_location("viewSize")?;
        let loc_tex = program.uniform_location("tex")?;
        let loc_masktex = program.uniform_location("masktex")?;
        let loc_frag = program.uniform_location("frag")?;

        Ok(Self {
            context: context.clone(),
            program,
            loc_viewsize,
            loc_tex,
            loc_masktex,
            loc_frag,
        })
    }

    pub(crate) fn set_tex(&self, tex: i32) {
        unsafe {
            self.context.uniform_1_i32(Some(&self.loc_tex), tex);
        }
    }

    pub(crate) fn set_masktex(&self, tex: i32) {
        unsafe {
            self.context.uniform_1_i32(Some(&self.loc_masktex), tex);
        }
    }

    pub(crate) fn set_view(&self, view: [f32; 2]) {
        unsafe {
            self.context.uniform_2_f32_slice(Some(&self.loc_viewsize), &view);
        }
    }

    pub(crate) fn set_config(&self, config: &[f32]) {
        unsafe {
            self.context.uniform_4_f32_slice(Some(&self.loc_frag), config);
        }
    }

    pub(crate) fn bind(&self) {
        self.program.bind();
    }

    pub(crate) fn unbind(&self) {
        self.program.unbind();
    }
}
