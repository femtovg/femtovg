# Changelog
All notable changes to this project will be documented in this file.

## Unreleased

### Fixed

 - Fixed erroneously multiply applied global alpha when mixing color glyphs with regular glyphs.

### Changed

 - MRSV was bumped to Rust 1.60, the crate now uses Rust Edition 2021.
 - `new_from_glutin_context` can now be used with headless contexts.
 - All const-safe `Color` constructors are now const.
 - `Canvas`'s text layout methods no longer require a mutable reference.
 - Removed the copy trait from `Paint` to avoid accidental copies.
 - `Paint` is always supplied by reference now.
## [0.3.6] - 2022-10-23

### Fixed

 - Fix build with latest rustybuzz release.

## [0.3.5] - 2022-05-23

### Changed

 - Optimized the OpenGL renderer to perform better on older GPUs by splitting the large fragment shader
   into smaller programs.

## [0.3.4] - 2022-04-07

### Added

 - Added support for importing backend-specific textures into the rendering of a scene with `Canvas::create_image_from_native_texture`.
 - Added functions to `TextContext` to configure the text shaping caches: `resize_shaping_run_cache` and `resize_shaped_words_cache`.

### Changed

 - Added optimized rendering code path for the common case of filling a rectangular path with an image and anti-aliasing
   on the paint disabled.

### Fixed

 - Fixed line breaking to permit a break in the middle of a word if it is the first word in the paragraph
   and it doesn't fit otherwise.

## [0.3.3] - 2022-02-21

### Changed

 - Bumped rustybuzz and ttf-parser dependencies.

## [0.3.2] - 2022-02-09

### Fixed

 - Correctly detect when WebGL is disabled in a web browser in the `renderer::OpenGL::new_from_html_canvas` function.

## [0.3.1] - 2022-02-08

### Fixed

 - Don't require default features of glutin. We don't need any and this way other users of glutin
   have the ability to opt out.

## [0.3.0] - 2022-02-04

### Changed

 - **Breaking:** The dependency to the `image` crate was bumped from `0.23` to `0.24`.
   Since the types of this crate are used in public femtovg API, users need to upgrade
   their dependency to the `image` crate as well.
 - **Breaking**: Removed deprecated `renderer::OpenGL::new` function. Use `renderer::OpenGl::new_from_function`
   or `renderer::OpenGl::new_from_glutin_context`.

### Added

 - Use `Paint::image_tint` to create an image paint that not only applies an alpha but an entire color (tint).

### Fixed

 - Improved performance of `fill_path` and `stroke_path`

[0.3.0]: https://github.com/femtovg/femtovg/releases/tag/v0.3.0
[0.3.1]: https://github.com/femtovg/femtovg/releases/tag/v0.3.1
[0.3.2]: https://github.com/femtovg/femtovg/releases/tag/v0.3.2
[0.3.3]: https://github.com/femtovg/femtovg/releases/tag/v0.3.3
[0.3.4]: https://github.com/femtovg/femtovg/releases/tag/v0.3.4
[0.3.5]: https://github.com/femtovg/femtovg/releases/tag/v0.3.5
