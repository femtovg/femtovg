# Setting Up

Femtovg uses OpenGL to talk to the GPU. We'll need to give Femtovg an [OpenGL context](https://www.khronos.org/opengl/wiki/OpenGL_Context) – an object that stores a bunch of stuff needed to draw things. Then, we can create a Canvas to draw things on!

## Creating an OpenGL Context

> If you're new to graphics, maybe this part will feel a bit overwhelming. Don't worry, we'll wrap all the weird code in a function and never worry about it again.

So, how do we get this OpenGL context? We'll use the `glutin` library to create a Window, as well as a context for rendering to that window:

```toml
[dependencies]
glutin = { version = "0.28.0", features = ["x11"] }
```

The first thing we need to do is create an Event Loop – we'll only really use it later, but we can't even create a window without it!

```rust
use glutin::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new();
}
```

Let's configure a window. We can specify many settings here, but let's just set the size and title:

```rust
use glutin::window::WindowBuilder;
use glutin::dpi::PhysicalSize;

let window_builder = WindowBuilder::new()
    .with_inner_size(PhysicalSize::new(1000., 600.))
    .with_title("Femtovg");
```

Now, we can create a context for this window – note that we pass `window_builder` as an argument:

```rust
use glutin::ContextBuilder;

let context = ContextBuilder::new()
    .with_vsync(false)
    .build_windowed(window_builder, &event_loop)
    .unwrap();
```

Finally, we have to make the context current:

```rust
let current_context = unsafe {
    context
        .make_current()
        .expect("Could not make the context current")
};
```

> In order for any OpenGL commands to work, a context must be current; all OpenGL commands affect the state of whichever context is current (*from [OpenGL wiki](https://www.khronos.org/opengl/wiki/OpenGL_Context)*)

We'll need the `event_loop` and `current_context` for the next step, but as promised, we can hide everything else in a function. Here's the code we have so far:

```rust
use glutin::dpi::PhysicalSize;
use glutin::event_loop::EventLoop;
use glutin::window::{Window, WindowBuilder};
use glutin::{ContextBuilder, ContextWrapper, PossiblyCurrent};

fn main() {
    let event_loop = EventLoop::new();
    let context = create_window(&event_loop);
}

fn create_window(event_loop: &EventLoop<()>) 
    -> ContextWrapper<PossiblyCurrent, Window> {
    let window_builder = WindowBuilder::new()
        .with_inner_size(PhysicalSize::new(1000., 600.))
        .with_title("Femtovg");
    let context = ContextBuilder::new()
        .with_vsync(false)
        .build_windowed(window_builder, &event_loop)
        .unwrap();

    let current_context = unsafe {
        context
            .make_current()
            .expect("Could not make the context current")
    };

    current_context
}
```

It compiles, runs, and immediately exits successfully.

## Creating a Canvas
We have an OpenGL context now – the Femtovg renderer can use it as output for rendering things. Let's create a renderer from the context we have:

```rust
let renderer = OpenGl::new_from_glutin_context(&context)
    .expect("Cannot create renderer");
```

The renderer is responsible for drawing things, but we can't draw on it directly – instead, we need to create a Canvas object:

```rust
let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
```

Finally, we have what we need to proceed to the next section – `canvas` has methods like `fill_path` and `fill_text` that actually draw stuff.