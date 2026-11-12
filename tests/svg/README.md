# SVG conformance tests

SVG files that `tools/svg-harness` renders with femtovg and compares with browsers. The layout follows resvg's
`crates/resvg/tests/tests`: `<category>/<feature>/<name>.svg`; reductions cited by an issue are in `issues/<N>/`.

There are no stored reference images. `tools/svg-harness/scripts/refs.py` renders references with Chromium,
Firefox and WebKit, plus an exact-area reference (Chromium at 8×, block-averaged). Browsers disagree at
antialiased edges, so each result names the reference it was compared with.

## Kinds of test

| Kind | Files | Passes when |
|---|---|---|
| Reftest | `name.svg` + `name-ref.svg` | femtovg renders both alike, as at least one browser engine does |
| Probe | `name.svg` | it is close to the browsers, read at each framing (zoom and frame size) |

A pair that every engine renders differently is reported as reference-inconsistent, not as a femtovg failure.

## Adding a test

- One feature or one bug per file, reduced to the fewest elements that still show it.
- Give the root `width`, `height` and `viewBox` unless the test is about their absence.
- No animation (SMIL or CSS): browser captures of animated files depend on timing.
- No text unless the test is about text. System fonts differ between machines.
- Use the browsers as the expectation. Check a new reftest pair in Chromium, Firefox and WebKit (`reftests.py`) before relying on it.

## Sources

- `filters`, `masking`, `painting`: pairs written for femtovg pull requests; several adapt WPT `css/compositing`,
  `css/filter-effects` and `css/masking` tests (BSD-3-Clause), the rest is MIT OR Apache-2.0. Each pair moved
  toward the browsers in recent releases, is still more than 1 % off a browser, or is cited in an issue.
- `issues/<N>`: reductions cited in issue #N, MIT OR Apache-2.0. A few adapt resvg tests (MIT OR Apache-2.0);
  `issues/424/shoe_merged.svg` is an SVGenius icon (Apache-2.0).

`reftests.py` judges a pair in Chromium, Firefox and WebKit. A pair that one engine renders apart still counts if
another renders it alike:

| Pair | Renders alike in |
|---|---|
| `blend-svg-root-isolation` | Firefox |
| `morphology-closing-chain` | WebKit |
| `blur-negative-and-zero-stddev`, `blur-primitive-subregion-clipping`, `clip-path-hidden-child`, `morphology-erode-complete` | Chromium, Firefox |

Some pairs differ by design in every engine (the `-ref` is not an equal rendering); they serve as two probes.

`external.json` lists files that are not redistributed here: Reddit, Google, Firefox, SolidJS and other logos,
model-drawn portraits, and larger files that issues cite (Babylon.js canvases, nine Ghostscript tigers, SVGenius
icons, an NYT crescent). They are fetched at a pinned revision with `scripts/corpus.py fetch` and never committed.
