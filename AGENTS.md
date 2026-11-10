# AGENTS.md

Guidance for contributors who use coding agents on femtovg.

CI builds selected native, wasm32 and feature configurations, runs unit and GPU tests, and checks that
`rustfmt` and `clippy` are clean. It does not check what most review rounds are about:

- whether a change renders closer to browsers
- what it costs per frame
- whether its explanation is true

This file covers those. The tools are in `tools/svg-harness` (renderer and scripts) and `tests/svg` (the corpus).

## Goal

femtovg aims to use as little CPU time, GPU work and memory as possible, and within those limits to render as
closely to the standards (SVG, Canvas 2D, CSS compositing) as browsers do. Every change is judged on both:

- A conformance gain must fit the budget of the median frame (see [Performance](#performance)).
- A saving must not move pixels away from the browsers (see [Screenshot and corpus comparisons](#screenshot-and-corpus-comparisons)).

## Writing: commits, comments, PR descriptions

- **Be concise.**
  - A commit message says what changed and why, in a few lines.
  - A PR description is about 250–400 words, in this order:
    1. summary (what, why, `Fixes #N`)
    2. evidence (images and tables)
    3. one line on tests
    4. one line on cost
  - A comment states the non-obvious *why* in one or two lines; the code says *what*.
- **Leave out the session.** Comments, commit messages and descriptions describe the code as it is. Leave out:
  - how the agent got there ("found while…", "first attempt…", "as discussed")
  - approaches that were tried and dropped
  - local build tags, dates, scratch paths, tool names
  - reviewer back-and-forth
  
  When understanding changes, rewrite the description in place. Do not append dated sections or follow-up comments.
- **Put numbers in tables, not prose.**
  - Performance and conformance results go in a Markdown table: one row per case or group, columns
    before | after | change, units in the header, fixed decimals.
  - Bold the cells that are major wins or losses, for example:
    - CPU cycles or instructions per frame
    - branch mispredictions and cache misses
    - allocations per frame
    - peak RSS or GPU footprint
    - pixels over threshold
    - reftests flipping between pass and fail
  - Leave noise unbolded, and say what noise is: the run-to-run spread.
  - The scripts in `tools/svg-harness/scripts` print tables already formatted this way.
- **Docs describe only what ships.**
  - A CHANGELOG entry goes under `## [Unreleased]` and says what users see.
  - A fix to behaviour that has not been released yet edits that feature's existing entry, or adds nothing.
- **Code review themes:**
  - Centralise duplicated tricky state.
  - Delete dead code and redundant early returns.
  - Prefer honest encodings (`Option<bool>` over a sentinel; an exhaustive `match` on a `#[non_exhaustive]` enum).
  - Make every degraded path observable (`bool` or `Result`).
  - Prove refactors with an old-vs-new A/B: bit-identical renders, or state the 1/255 rounding.

## Before you push

```bash
cargo fmt --all -- --check
cargo clippy --all-features --no-deps
cargo test --features wgpu
cargo check --lib --features wgpu --target wasm32-unknown-unknown
```

- **Verify the affected backend and features ran.** Use `FEMTOVG_REQUIRE_GPU=<backend>` for WGPU tests
  (for example, `metal` or `vulkan`). Record the adapter and executed tests. Test the actual GL/GLES path for
  claims about it. For feature-gated changes, run the relevant test configuration as well as building it.
- **wasm32 check.** Run it after any edit near `#[cfg(wasm_unknown)]` code (`copy_external_image`, the
  `HtmlImageElement`/`HtmlCanvasElement` arms of `update_image`). Native builds never compile those paths.
- **Clippy.** Compare its warnings with `master`'s before assuming they are yours.
- **Updating a branch.** Merge `master` into it; never rebase or force-push a branch under review.
  - Fetch into a named ref (`git fetch upstream master:refs/remotes/upstream/master`), not `FETCH_HEAD`, which the
    next fetch replaces.
  - Check `git merge-base --is-ancestor upstream/master HEAD` before pushing.

## What counts as evidence

- **Cross-check Chromium, Firefox and WebKit.** Use the spec for requirements it defines. If engines disagree
  and the spec leaves room, report the split and which behavior femtovg targets. Name the browser version, OS
  and rendering mode. macOS `webkit` uses system WKWebView and Linux `webkitgtk` uses WebKitGTK. Record Safari
  running on the physical device separately, with its OS and Safari versions. Capture the native femtovg
  output on that device separately when qualifying device rendering.
- **Name the reference.** Browsers disagree at antialiased edges:

  | Renderer | At an edge where clip and content meet |
  |---|---|
  | Chromium (software), Firefox | multiply the coverages; snap path edges to ¼ px |
  | Chromium (GPU) | covers the edge once in some cases; the tests are float-exact and per tile, so it varies with zoom |
  | macOS WebKit (observed) | multiplies coverages |

  `refs.py area` estimates coverage by averaging supersampled Chromium GPU output (8×, or 4× for wider frames).
- **Find the root cause, not the symptom.** A fix that "reduces differing pixels" without saying why is a patch.
  - Find the mechanism with a probe that varies one thing at a time: DPR, rotation, sub-pixel offset, zoom.
  - Ground the fix in a reference implementation's source: Skia, Firefox, WebKit, or NanoVG for inherited code.
  - Delete the band-aids it supersedes.
- **Regression tests fail without the fix.** Revert the fix and watch the test fail. Test the whole property:
  - width *and* position
  - every variant of an enum
  - uploads *and* render targets
  - DPR scaling

## Screenshot and corpus comparisons

Install the [harness prerequisites](tools/svg-harness/scripts/README.md). Run from the repository root with
`upstream/master` available. Set `SVG_GROUP=painting` for a smoke run on a few checked-in pairs.
Run on both hosts when comparing the two WebKit ports.

```bash
(
set -e
cd tools/svg-harness/scripts
group="group=${SVG_GROUP:-}"
if [ -z "${SVG_GROUP:-}" ]; then
  python3 corpus.py fetch                          # third-party sets in tests/svg/external.json
fi
python3 build.py before upstream/master            # same harness source, library at master
python3 build.py after                             # working tree
python3 refs.py chromium "$group"
python3 refs.py firefox "$group"
case "$(uname -s)" in
  Darwin) webkit_ref=webkit; python3 refs.py webkit "$group" ;;
  Linux)  webkit_ref=webkitgtk
          GDK_BACKEND=x11 xvfb-run -a -s '-screen 0 2560x1440x24' python3 refs.py webkitgtk "$group" ;;
  *) echo "Run the WebKit reference step on macOS or Linux" >&2; exit 1 ;;
esac
python3 run.py before after "$group"               # every selected file x framing
python3 compare.py before after "$group" "refs=chromium,firefox,$webkit_ref"
python3 reftests.py before after "$group"          # pairs judged in Chromium, Firefox and WebKit
)
```

- **Tie evidence to its inputs.** Record before/after revisions, harness and corpus revisions, and framing/DPR.
  Keep the same harness, inputs and fonts on both sides. Use a fresh `SVG_HARNESS_OUT` or invalidate affected
  reference PNGs after changing browsers, inputs, fonts or framing. A new version stamp can accompany old PNGs.
- **Five columns, every time.** Each accuracy or conformance claim carries the sheet:
  - Columns: before | after | reference | before vs reference | after vs reference.
  - Diff panels mark pixels more than 20/255 off in red and 1–20/255 in orange.
  - Name the reference and framing. Generate one sheet per relevant reference with
    `python3 sheet.py out.png before after KEY@z1 KEY@hd refs=webkitgtk zoom=8` (substitute the reference).
  - Keep the sheets in the PR description and regenerate them after rendering-relevant revisions.
  - A diff between the two builds alone, or an after-only image, does not show whether the change helped.
- **Lead with what a viewer sees.** Report 1× and 1080p first.
  - Sub-1× and extreme zooms find mechanisms; they rarely justify a PR alone.
  - If a fix moves only a few pixels at normal sizes, say so in the first line.
- **Explain every frame that moves** before writing why it moved:
  - **Size.** How many pixels move, and by how much? Rounding means 1–2/255, not 10+.
  - **Reach.** Can the change reach that content at all? Use `--without text,clip,blend,shadow,turbulence,filters`
    to localise it.
  - **Direction.** Did it move toward or away from each reference? Moving away from all of them is a bug until
    shown otherwise.
  - **Same harness.** `build.py` copies one harness into both builds, because a harness difference looks exactly
    like a library change.
- **Check the reference before blaming the renderer.**
  - `refs.py` frames pages with usvg's size rules (width/height vs viewBox, `preserveAspectRatio`, units, sizeless
    roots).
  - Open each group's worst frames after a run: an icon more than 5 % off with no unsupported feature is usually
    framing or parsing.
  - Animated (SMIL) files are excluded from metrics.
- **After changing the harness**, sample known pixels against the references. Summary numbers can stay flat for
  the wrong reason.

### Conformance suites

- **Check both halves of disputed pairs in each engine.** Compare test vs. reference at the same framing
  within Chromium, Firefox and WebKit. Report missing, excluded and reference-inconsistent cases separately
  from passes. State both the per-pixel error threshold and the allowed percentage of affected pixels.
- **Group failures by root cause**, one issue per cause, each with its sheet.
- **Reduce each failure to a minimal SVG** under `tests/svg/<category>/<feature>/`; it becomes the regression test.
- **Check 1×, 2× and 4×.** Many edge and filter bugs show at one scale only.
  Use `python3 zoom.py after FILE` for a zoom-in/out sequence on one persistent canvas.

## Performance

- **The budget is the median frame.** Typical frames are UI frames with many small fills, on hardware down to a
  Raspberry Pi 3B. A change that helps the tail but raises the median needs a fast path or an opt-in.
- **Per-draw work stays behind the early-out tests.**
  - Work placed before the "does the clip hold this draw" test runs for every fill (+14 % instructions on a page
    of unclipped cards).
  - Optional fragment work gets its own shader variant, not a uniform branch (+17 % on OpenGL unclipped fills).
  - On wgpu, a binding added to a per-draw bind group costs every draw.

How to measure:

```bash
python3 timing.py before after FILE...         # paired A/B/A/B runs of `svg-harness time`, frame 1 excluded
python3 timing.py instr before after FILE...   # instructions and cycles per frame, long minus short run
```

- **Paired, alternating runs.** Use at least 14 pairs of 200+ frames; single medians mislead by 1–3 %.
- **Check `uptime` first.** Indexing daemons woken by thousands of new PNGs skew medians by 1–2 %.
- **Counters over wall time.**
  - Instructions are steady to 0.1–0.5 % for one binary, but carry 1–2 % code-layout noise between two builds.
    Below that, benchmark the function in a loop.
  - The real costs are cache misses, branch mispredictions and GPU tile work; measure them directly.
- **Startup and pacing.** Count startup in a fresh process up to a named first-paint marker (render, submit or
  present). For scheduling changes, report p95/p99 frame intervals as well as medians.
- **Sampling-profiler shares are not time shares.** `/usr/bin/sample` once gave 29–48 % to a lookup whose removal
  saved 9 %. Confirm a share with an A/B of a build without the cost.
- **Performance-only PRs show no rendering diffs.** Give the timing table and state that all frames are
  bit-identical (`compare.py`).
- **GPU memory:** RSS misses Metal and driver memory.
  - `svg-harness render --stats` reports the macOS physical and graphics footprint.
  - On iOS, read `rpages` in the jetsam report.
  - Scale a synthetic input and measure the peak, then drain GPU work and tear down to check what remains.

### CPU profiling and hardware counters

- **Goal:** explain a cost by its cause (instructions, branch mispredictions, L1/TLB misses per frame), then
  confirm the cause with a paired A/B. Counters are evidence; profiler percentages are leads.
- **Build:** release with symbols, same features and flags on both sides; sanitizers in separate runs.
- **Scope:** check the events came from the right process, threads and phase (steady state, not startup), and
  say when events were multiplexed or unsupported. Report per frame, with IPC and misses per 1,000 instructions.
- **Intel:** VTune `uarch-exploration`, then `memory-access` if it points at memory; one result dir per run.
- **Apple:** Instruments CPU Counters in manual mode. On hybrid chips pin to efficiency cores with
  `taskpolicy -c background`, record each build twice in alternating order (cycles vary ~13 % between runs),
  and delete the trace and the multi-GB `instruments*.ktrace` in `$TMPDIR` afterwards.
- **Linux:** `perf stat -e instructions,cycles,branch-misses,...` with the same paired runs.

## Fuzzing and property tests

- **Seed them so `cargo test` runs them.** Examples: `svg_arc_deterministic_property_fuzz`,
  `random_api_sequences_keep_one_consistent_stack`. Print the seed and case on failure.
- **Sort findings by kind:**
  - crash or abort
  - memory or time blowup (near-singular transforms, huge filter regions)
  - wrong pixels

  Each kind needs its own fix and regression test.
- **Minimise first.** A degenerate input (zero-length cubic, zero scale, `skewX(89.999)`) usually points at one
  missing guard.
- **Test the real property:** in device space, across zooms.
- **File findings outside the change's scope as issues.**

## Code coverage

- `cargo llvm-cov --features wgpu` shows untested branches in the code you touched. Look first at fallback paths:
  failed layer allocation, a full atlas, an exhausted pool.
- Each degraded path needs a test that forces it.
- For queues, pools, layers, filters and clips, test just below, at and above limits. Assert resource bounds
  and pixels across flushes and repeated frames. Mix nested layers, filter chains and clip/scissor restoration.
  For submission changes, include a caller-owned prepass and verify execution order.
- Coverage of the OpenGL backend needs a GL context. On macOS, use an invisible window and an image target.

## Agent hygiene

- **Presence-gated environment switches.** `VAR= cmd` still sets the variable, so the switch is on. Run the off
  case with `env -u VAR`, and check that on and off differ somewhere before trusting "identical".
- **Shared `CARGO_TARGET_DIR` across worktrees** can link the other worktree's femtovg. Run
  `cargo clean --release -p femtovg` before building elsewhere (`build.py` does).
- **Parallel jobs** each need their own output file.
- **Resumable result files** must not outlive a rebuild of their binary.
- **Disk.**
  - A `cargo test --features wgpu` debug build is ~6 GB per worktree, ~2.6 GB with
    `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`.
  - Use one target dir per session and delete `debug/` after test runs.
  - Check `df` before recordings and parallel builds.
- **Gate pushes with `if … then … else … fi`.** In `a && b || push`, the `||` catches any failure in the chain.
