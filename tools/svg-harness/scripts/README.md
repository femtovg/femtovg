# svg-harness scripts

Accuracy and cost of a femtovg change, measured on the SVG corpus against browser references. The scripts drive
the `svg-harness` binary (`tools/svg-harness`) and the browsers; every report is a set of Markdown tables, ready
for a pull request.

Requirements: Python 3.9 or later with Pillow and numpy, a Rust toolchain, headless Chromium
(`npx @puppeteer/browsers install chrome-headless-shell@stable`), Firefox, and a working WGPU adapter.
Set `CHROMIUM` or `FIREFOX` to the executable if discovery fails.

- macOS WebKit needs Xcode command-line tools for `swiftc`.
- Debian 13 packages for headless Linux include `build-essential pkg-config libwebkit2gtk-4.1-dev xvfb xauth
  chromium firefox-esr python3-pil python3-numpy libfontconfig1-dev libvulkan1 mesa-vulkan-drivers`. Mesa Vulkan
  provides a software adapter for the renderer smoke test. Run browsers as an ordinary user. In Docker, start the
  container with `--security-opt seccomp=unconfined`: Chromium's sandbox needs user namespaces, which the default
  profile blocks.

## A typical A/B

```sh
cd tools/svg-harness/scripts
./corpus.py fetch                      # third-party sets into target/svg-corpus (once)
./build.py base upstream/master        # the library at a rev, with the current harness
./build.py mine                        # the library in the working tree
./refs.py chromium
./refs.py firefox
case "$(uname -s)" in
  Darwin) ./refs.py webkit ;;
  Linux)  GDK_BACKEND=x11 xvfb-run -a -s '-screen 0 2560x1440x24' ./refs.py webkitgtk ;;
esac
./run.py base mine                     # render and compare every file at z1, z2, z4, hd
./compare.py base mine                 # what moved, toward or away from each reference
./reftests.py base mine                # X.svg vs X-ref.svg pairs
./sheet.py out.png base mine masking__mask__mask-negative-scale@z2 zoom=8
./timing.py base mine external__buseybench__corpus__buseybench__gpt-6-astra
```

Narrow any run with `group=` (a group such as `painting/mix-blend-mode`, or a prefix such as `masking` or `external`) or
`keys=FILE` (one key per line). Outputs go to `target/svg-harness` (`SVG_HARNESS_OUT` to change it):

| path | what |
|:---|:---|
| `builds/NAME/` | `svg-harness` and `build.json` (rev, local changes, date) |
| `refs/BROWSER/KEY@FRAMING.png` | references, and `version.txt` with the browser's version |
| `frames/BUILD/KEY@FRAMING.png` | frames kept by `run.py` or rendered by `compare.py`/`sheet.py` |
| `results/BUILD.jsonl` | one row per frame: pixel hash, `--stats`, metrics per reference |
| `zoom/`, `soak/` | `zoom.py` animations, `timing.py soak` CSVs |

## Scripts

| script | does |
|:---|:---|
| `common.py` | paths, framings, thresholds, builds, browsers, metrics and tables; run it to print the framings and thresholds |
| `corpus.py` | `fetch` the external sets, `list` files, `groups` with counts |
| `page.py` | the browser page for one file at one framing |
| `build.py` | build the harness against the working tree or a rev |
| `refs.py` | browser references: `chromium`, `chromium-gpu`, `firefox`, `webkit`, `webkitgtk`, `area` |
| `run.py` | render and measure builds over the corpus |
| `compare.py` | two builds' results: identical frames, moves per reference, groups, worst frames, area table, stats |
| `reftests.py` | reftest pass counts, flips between two builds, reference-inconsistent pairs |
| `sheet.py` | the five-column evidence sheet: before, after, reference, before vs reference, after vs reference |
| `zoom.py` | a zoom gesture on one canvas against the references, as a table and a GIF/WebP animation |
| `timing.py` | paired frame times, instruction counts, and soak summaries |
| `wk_shot.swift`, `wkgtk_shot.c` | macOS WKWebView and Linux WebKitGTK screenshot helpers (compiled on first use) |

Each script prints its usage with `-h`.

## Corpus

The corpus is `tests/svg/<category>/<feature>/*.svg`; a reftest is `NAME.svg` with `NAME-ref.svg` beside it. The
third-party sets in `tests/svg/external.json` are fetched shallow and sparse at their pinned revs into
`target/svg-corpus/<set>/` (or used in place when `local`), never committed. A file's key is its path below
`tests/svg` (or `external/<set>/...`) without `.svg`, with `/` as `__`; its group is the first two components.
Files with SMIL animation are flagged and left out of the counts, since their frames depend on time.

## Framing

The harness and the browser pages draw the same scene: a frame, the SVG's size fitted into a square box, and a zoom
about a pivot (`svg-harness render --frame WxH --box SIZE@X,Y --zoom Z --pivot X,Y`). `page.py` reads the root's
width, height, viewBox and preserveAspectRatio as usvg does, so a viewBox whose aspect differs from the viewport's
is letterboxed on both sides alike. A framing mismatch shows up as a whole-frame shift; check it before reading a
difference as a regression.

| framing | frame | box | zoom |
|:---|:---|:---|:---|
| `z1`, `z2`, `z4` (default) | 460x260 | 200 at 130,30 | 1, 2, 4 about the box centre |
| `hd` (default) | 1920x1080 | 1080 at 420,0 | 1 |
| `z01` ... `z3` | 460x260 | 200 at 130,30 | 0.1, 0.25, 0.33, 0.5, 0.75, 1.25, 1.5, 1.75, 3 |
| `zt2`, `zt4` | 460x260 | 200 at 130,30 | 2, 4 about 230,48, for wide artwork |
| `p240` | 426x240 | 240 at 93,0 | 1 |

## References

Browsers disagree at antialiased edges, so name the reference a result is measured against:

- `chromium`: software rasterization, deterministic; snaps path edges to a quarter pixel.
- `chromium-gpu`: GPU rasterization, as desktop Chrome draws.
- `firefox`: headless Firefox.
- `webkit`: macOS system WebKit via WKWebView. Record physical-device Safari screenshots separately.
- `webkitgtk`: Linux WebKitGTK 4.1, under Xvfb on a headless host.
- `area`: a coverage estimate from supersampled Chromium GPU output, averaged down.

## Metrics and reports

A pixel differs when a channel is more than 20/255 off; a frame's metric is the share of its pixels that do
(`px`), with the share 1..20/255 off (`faint`) and the largest difference alongside. Thresholds live in
`common.py`. Reports bold the changes that matter:

| measure | bold when |
|:---|:---|
| accuracy | a frame or group moves by 1 percentage point of pixels or more; a reftest flips |
| time | the median ratio is more than 3 % from 1 and the spread of the pairs stays on one side |
| memory | 10 % or more |

## Getting numbers you can trust

- `build.py` builds both sides of an A/B with one harness revision: frames moving where the library change cannot
  reach usually mean the harness differed. It cleans `femtovg` and `svg-harness` before each build, since with a
  shared target directory cargo can link a library built from another tree, and checks that the tree's wgpu
  shaders are in the binary.
- Results and kept frames are tagged with the binary's rev and mtime; after a rebuild, `run.py` drops the old rows.
- `timing.py` alternates the builds and reports frame 1 apart from the steady state. It warns when the load
  average is high; background work shifts a median by a few percent. For small differences, `timing.py instr`
  counts instructions instead.
- Peak memory on the GPU lives in the process footprint (graphics memory), not in the resident set.
