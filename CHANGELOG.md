# Changelog
All notable changes to this project will be documented in this file.

## Unreleased

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
