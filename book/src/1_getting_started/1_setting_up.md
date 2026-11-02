# Setting Up

Femtovg uses OpenGL to talk to the GPU. We'll need to give Femtovg an [OpenGL context](https://www.khronos.org/opengl/wiki/OpenGL_Context) – an object that stores a bunch of stuff needed to draw things. Then, we can create a Canvas to draw things on!

## Creating an OpenGL Context

> If you're new to graphics, maybe this part will feel a bit overwhelming. Don't worry, we'll wrap all the weird code in a function and never worry about it again.

So, how do we get this OpenGL context? We'll use the winit library to create a window and the `glutin` library to create an OpenGL context for rendering to that window. `glutin-winit` connects the two, and `raw-window-handle` lets glutin find the window winit made:

```toml
[dependencies]
femtovg = "0.28"
winit = "0.30"
glutin = "0.32"
glutin-winit = "0.5"
raw-window-handle = "0.6"
```

The first thing we need to do is create an Event Loop. winit hands it an application – a type implementing `ApplicationHandler` – and calls it back as things happen. Windows can only be created once the application has been resumed, so that is where we'll set everything up:

```rust,ignore
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};

struct App {
    context: Option<PossiblyCurrentContext>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.context.is_none() {
            self.context = Some(create_window(event_loop));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: winit::window::WindowId, event: WindowEvent) {}
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { context: None };
    event_loop.run_app(&mut app).unwrap();
}
```

Let's configure a window. We can specify many settings here, but let's just set the size and title:

```rust,ignore
use winit::window::Window;
use winit::dpi::PhysicalSize;

let window_attrs = Window::default_attributes()
    .with_inner_size(PhysicalSize::new(1000., 600.))
    .with_title("Femtovg");
```

Next we specify a configuration for that window. Usually windows may have many different properties. Think transparency, OpenGL support, bit depth. The following lines find one that is suitable for rendering:

```rust,ignore
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;

use glutin::{
    config::ConfigTemplateBuilder,
    context::ContextAttributesBuilder,
    context::PossiblyCurrentContext,
    display::GetGlDisplay,
    prelude::*,
};

let template = ConfigTemplateBuilder::new().with_alpha_size(8);

let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attrs));

let (window, gl_config) = display_builder
    .build(event_loop, template, |mut configs| configs.next().unwrap())
    .unwrap();

let window = window.unwrap();

let gl_display = gl_config.display();

let raw_window_handle = window.window_handle().unwrap().as_raw();
let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));

let mut not_current_gl_context =
    Some(unsafe { gl_display.create_context(&gl_config, &context_attributes).unwrap() });

```

Now, we can create a surface for rendering and make our OpenGL context current on that surface:

```rust,ignore
use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};

let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
    raw_window_handle,
    NonZeroU32::new(1000).unwrap(),
    NonZeroU32::new(600).unwrap(),
);

let surface = unsafe { gl_config.display().create_window_surface(&gl_config, &attrs).unwrap() };

not_current_gl_context.take().unwrap().make_current(&surface).unwrap()
```

> In order for any OpenGL commands to work, a context must be current; all OpenGL commands affect the state of whichever context is current (*from [OpenGL wiki](https://www.khronos.org/opengl/wiki/OpenGL_Context)*)

We'll need the current context for the next step, but as promised, we can hide everything else in a function, `create_window`. Here's the code we have so far:

```rust,ignore
{{#include 1_setting_up.rs}}
```

It compiles, runs, and opens an empty window that closes when you click its close button.

## Creating a Canvas
We have an OpenGL context and display now – the Femtovg renderer can use it as output for rendering things. Let's create a renderer from the display we have:

```rust,ignore
let renderer = unsafe { OpenGl::new_from_function_cstr(|s| gl_display.get_proc_address(s).cast()) }
        .expect("Cannot create renderer");
```

The renderer is responsible for drawing things, but we can't draw on it directly – instead, we need to create a Canvas object:

```rust,ignore
let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
```

Finally, we have what we need to proceed to the next section – `canvas` has methods like `fill_path` and `fill_text` that actually draw stuff.