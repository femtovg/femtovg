# Changelog
All notable changes to this project will be documented in this file.

## Unreleased

## [0.9.2] - 2024-06-27

 - Fix path rendering where the default path solidity would interfere with the path's
   own winding direction (https://github.com/femtovg/femtovg/issues/124)
 - Fix blurry text rendering when drawing on non-integer coordinates
 - Bumped MSRV to 1.68.

## [0.9.1] - 2024-04-12

 - Fixed inability to introspect `Path` verbs by making `PathIter` and `Verb` public.
 - Fixed rendering of text strokes with large font sizes.

## [0.9.0] - 2024-02-27

 - **breaking**: Removed pub key field in ImageId. This accidentally
   exposed the implementation detail of the image store (generational-arena),
   which has been replaced with slotmap.
 - For WASM builds, require WebGL 2. This is supported by all major browsers
   and needed to make `ImageFlags::REPEAT_X/Y` work.
 - Bumped MSRV to 1.66.

## [0.8.2] - 2024-01-20

 - Improved performance when rendering large texts.
 - Replace error logging to stderr with use of log crate.

## [0.8.1] - 2023-12-18

 - Fix documentation build on docs.rs.

## [0.8.0] - 2023-11-02

 - Re-release 0.7.2 with major version bump. 0.7.2 was yanked because
   glow is a re-exported public dependency, that was bumped.

## [0.7.2] - 2023-11-02

 - Bump internal dependencies.

## [0.7.1] - 2023-06-14

- Fix performance regression when drawing unclipped image path fills.

## [0.7.0] - 2023-05-26

### Changed

 - Path drawing functions now take a `&Path` instead of a `&mut Path` and use interior mutability
   for caching.

## [0.6.0] - 2023-02-06

### Changed

 - Changed `linear_gradient_stops` and `radial_gradient_stops` to take an `IntoIterator`
   instead of a slice slice for the color stops.

## [0.5.0] - 2023-02-06

### Added

 - added a new `Size` struct, having a `width` and a `height`.
 - added `size` function to `Image` type, which returns both, `width` and `height` as a `Size`

### Changed

 - Renamed `draw_glyph_cmds` to `draw_glyph_commands`.
 - Renamed `DrawCmd` to `DrawCommand`.
 - `set_transform` takes a value of type `Transform2D` now instead of a parameter list.
 - `dimensions` of `ImageSource` returns a new `Size` type now.

## [0.4.0] - 2023-01-27

### Added

 - `OpenGl::new_from_function_cstr` to create the renderer from a GL loading function that
   takes an `&std::ffi::CStr`.

### Fixed

 - Fixed erroneously multiply applied global alpha when mixing color glyphs with regular glyphs.

### Changed

 - MRSV was bumped to Rust 1.63, the crate now uses Rust Edition 2021.
 - `new_from_glutin_context` can now be used with headless contexts.
 - All const-safe `Color` constructors are now const.
 - `Canvas`'s text layout methods no longer require a mutable reference.
 - Removed the copy trait from `Paint` to avoid accidental copies.
 - `Paint` is always supplied by reference now.
 - `TextContext`'s `resize_shaping_run_cache` and `resize_shaped_words_cache` functions now take a
   `std::num::NonZeroUsize` for the capacity value.
 - As part of the glutin update, `OpenGL::new_from_glutin_context` was renamed to `new_from_glutin_display` and takes a glutin display now.
 - Removed `glutin` from the default features.

## [0.3.7] - 2022-10-24

### Fixed

 - Fix build with latest rustybuzz release after 0.5.2 breakage. 0.5.3 doesn't
   re-export the ttf_parser module anymore.

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
[0.3.6]: https://github.com/femtovg/femtovg/releases/tag/v0.3.6
[0.3.7]: https://github.com/femtovg/femtovg/releases/tag/v0.3.7
[0.4.0]: https://github.com/femtovg/femtovg/releases/tag/v0.4.0
[0.5.0]: https://github.com/femtovg/femtovg/releases/tag/v0.5.0
[0.6.0]: https://github.com/femtovg/femtovg/releases/tag/v0.6.0
[0.7.0]: https://github.com/femtovg/femtovg/releases/tag/v0.7.0
[0.7.1]: https://github.com/femtovg/femtovg/releases/tag/v0.7.1
[0.7.2]: https://github.com/femtovg/femtovg/releases/tag/v0.7.2
[0.8.0]: https://github.com/femtovg/femtovg/releases/tag/v0.8.0
[0.8.1]: https://github.com/femtovg/femtovg/releases/tag/v0.8.1
[0.8.2]: https://github.com/femtovg/femtovg/releases/tag/v0.8.2
[0.9.0]: https://github.com/femtovg/femtovg/releases/tag/v0.9.0
[0.9.1]: https://github.com/femtovg/femtovg/releases/tag/v0.9.1
