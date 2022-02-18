# Rendering

Now that we have a `canvas`, we can start drawing things! To keep things organized, let's create a `render` function that will do all the rendering:

```rust
fn main() {
    // [...]
    let mut canvas = ..;

    render(&context, &mut canvas);
}

fn render<T: Renderer>(
    context: &ContextWrapper<PossiblyCurrent, Window>,
    canvas: &mut Canvas<T>
) {}
```

In `render`, first let's make sure that the canvas has the right size – it should match the dimensions and DPI of the window:

```rust
let window = context.window();
let size = window.inner_size();
canvas.set_size(size.width, size.height, window.scale_factor() as f32);
```

Next, let's do some actual drawing. As an example, we'll fill a smol red rectangle:

```rust
canvas.clear_rect(30, 30, 30, 30, Color::rgbf(1., 0., 0.));
```

> [`clear_rect`](https://docs.rs/femtovg/latest/femtovg/struct.Canvas.html#method.clear_rect) fills a rectangle. The first 2 parameters specify its position, and the next 2 specify the dimensions of the rectangle.
> 
> [`Color::rgbf`](https://docs.rs/femtovg/latest/femtovg/struct.Color.html#method.rgbf) is one of the functions that lets you create a Color. The three parameters correspond to the Red, Green and Blue values in the range 0..1.

Even if you consider your minimalist abstract masterpiece complete, there's actually some more code we need to write. We have to call [`canvas.flush()`](https://docs.rs/femtovg/latest/femtovg/struct.Canvas.html#method.flush) to tell the renderer to execute all drawing commands. Then, we must call [`swap_buffers`](https://docs.rs/glutin/latest/glutin/struct.ContextWrapper.html#method.swap_buffers) to display what we've rendered: 

```rust
context.swap_buffers().expect("Could not swap buffers");
```

The `render` function is finished, but if you run your program, you won't get to look at it for very long – as soon as `render` completes, the program exits. To fix this, let's freeze the program with an infinite `loop {}`.

Our program now looks like this:

```rust
use femtovg::renderer::OpenGl;
use femtovg::{Canvas, Color, Renderer};
use glutin::dpi::PhysicalSize;
use glutin::event_loop::EventLoop;
use glutin::window::{Window, WindowBuilder};
use glutin::{ContextBuilder, ContextWrapper, PossiblyCurrent};

fn main() {
    let event_loop = EventLoop::new();
    let context = create_window(&event_loop);

    let renderer = OpenGl::new_from_glutin_context(&context)
        .expect("Cannot create renderer");
    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");

    render(&context, &mut canvas);

    loop {}
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

fn render<T: Renderer>(
    context: &ContextWrapper<PossiblyCurrent, Window>,
    canvas: &mut Canvas<T>
) {
    // Make sure the canvas has the right size:
    let window = context.window();
    let size = window.inner_size();
    canvas.set_size(size.width, size.height, window.scale_factor() as f32);

    // Make smol red rectangle
    canvas.clear_rect(30, 30, 30, 30, Color::rgbf(1., 0., 0.));

    // Tell renderer to execute all drawing commands
    canvas.flush();
    // Display what we've just rendered
    context.swap_buffers().expect("Could not swap buffers");
}
```

And when we run it, we see the red square we rendered:

![Window titled Femtovg containing a small red square on a black background](2_app.png)