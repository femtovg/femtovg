//! Renders SVG files through usvg into femtovg's wgpu backend, headless, for conformance comparisons against
//! browser references and for timing and memory runs. See `svg-harness --help`.

mod clip;
mod filters;
mod gpu;
mod scene;
mod stats;
mod svg;
mod text;

use std::process::ExitCode;

use femtovg::Color;

use crate::{
    gpu::{CanvasOptions, Session},
    scene::SceneOptions,
    stats::{mem, JsonLine},
    text::TextMode,
};

const USAGE: &str = "\
Renders SVG files through usvg and femtovg's wgpu backend, headless.

Usage:
  svg-harness render [options] IN.svg OUT.png
  svg-harness time [options] [--frames N] IN.svg
  svg-harness ladder [options] --zooms Z1,Z2,... IN.svg OUTPREFIX
  svg-harness soak [options] [--passes N] [--zooms Z1,Z2,...] LIST.txt OUT.csv

Commands:
  render   Draws one frame and writes it as an RGBA8 PNG.
  time     Draws N frames (default 200) on one canvas and prints one JSON line to stdout: frame 1's
           time (pipeline compiles and atlas creation included), then the median, 95th percentile and
           minimum over frames 2..N of the encode time (scene walk and flush) and the wall time (to
           GPU completion), and the run's user and system CPU seconds.
  ladder   Draws one frame per zoom on one persistent canvas, as a zoom gesture does, and writes
           OUTPREFIX.00.png, OUTPREFIX.01.png, ...
  soak     Draws every SVG listed in LIST.txt (one path per line) at each zoom (default 1,2,4) for
           N passes (default 3) on one canvas, and writes one CSV row per frame with its timings and
           memory.

Framing:
  --frame WxH            Frame size in pixels [default: 460x260]
  --box SIZE@X,Y         The square the SVG's size is fitted into [default: 200@130,30]
  --zoom Z               Zoom about the pivot [default: 1]
  --pivot X,Y            Zoom pivot in frame pixels [default: the box centre]
  --dark                 Dark background (32, 34, 37) instead of white
  --no-viewport-clip     Let content outside the SVG's viewport show

Canvas:
  --transient-budget-mb N    Transient image budget [default: 1024]
  --filter-work-gsamples N   Filter work budget in Gi samples [default: the library's]
  --max-texture-size N       Lower the device's texture dimension limit

Drawing:
  --text MODE            outlines: usvg's glyph outlines as paths [default]
                         glyphs: usvg's layout, glyphs through femtovg's glyph runs
                         shape: each text chunk through femtovg's shaping
  --without LIST         Leave out, comma separated: text, clip, blend, shadow, turbulence, filters

Output:
  --stats                Print one JSON line of counters, timings and peak memory to stderr
  --tree                 Print the usvg tree to stderr (render)
  --tree-size            Print the size usvg resolved for the root and stop (render; OUT.png optional)
  -h, --help             Print this help
";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Render,
    Time,
    Ladder,
    Soak,
}

struct Args {
    command: Command,
    scene: SceneOptions,
    canvas: CanvasOptions,
    zoom: f32,
    stats: bool,
    tree: bool,
    tree_size: bool,
    frames: usize,
    passes: usize,
    zooms: Option<Vec<f32>>,
    positional: Vec<String>,
}

fn parse_number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{flag}: '{value}' is not a valid number"))
}

fn parse_pair(flag: &str, value: &str, separator: char) -> Result<(f32, f32), String> {
    let (a, b) = value
        .split_once(separator)
        .ok_or_else(|| format!("{flag}: expected two numbers separated by '{separator}', got '{value}'"))?;
    Ok((parse_number(flag, a)?, parse_number(flag, b)?))
}

fn parse_args(raw: Vec<String>) -> Result<Args, String> {
    let mut raw = raw.into_iter();
    let command = match raw.next().as_deref() {
        Some("render") => Command::Render,
        Some("time") => Command::Time,
        Some("ladder") => Command::Ladder,
        Some("soak") => Command::Soak,
        Some(other) => return Err(format!("unknown command '{other}'")),
        None => return Err("no command given".into()),
    };
    let mut args = Args {
        command,
        scene: SceneOptions::default(),
        canvas: CanvasOptions::default(),
        zoom: 1.0,
        stats: false,
        tree: false,
        tree_size: false,
        frames: 200,
        passes: 3,
        zooms: None,
        positional: Vec::new(),
    };
    while let Some(arg) = raw.next() {
        if !arg.starts_with("--") || arg == "--" {
            args.positional.push(arg);
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
            None => (arg, None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| raw.next())
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--frame" => {
                let (w, h) = parse_pair(&flag, &value()?, 'x')?;
                args.scene.frame = (w as u32, h as u32);
            }
            "--box" => {
                let v = value()?;
                let (size, at) = v
                    .split_once('@')
                    .ok_or_else(|| format!("--box: expected SIZE@X,Y, got '{v}'"))?;
                args.scene.box_size = parse_number(&flag, size)?;
                args.scene.box_at = parse_pair(&flag, at, ',')?;
            }
            "--zoom" => args.zoom = parse_number(&flag, &value()?)?,
            "--pivot" => args.scene.pivot = Some(parse_pair(&flag, &value()?, ',')?),
            "--dark" => args.scene.background = Color::rgb(32, 34, 37),
            "--no-viewport-clip" => args.scene.viewport_clip = false,
            "--transient-budget-mb" => args.canvas.transient_budget_mb = parse_number(&flag, &value()?)?,
            "--filter-work-gsamples" => args.canvas.filter_work_gsamples = Some(parse_number(&flag, &value()?)?),
            "--max-texture-size" => args.canvas.max_texture_size = Some(parse_number(&flag, &value()?)?),
            "--text" => {
                args.scene.text = match value()?.as_str() {
                    "outlines" => TextMode::Outlines,
                    "glyphs" => TextMode::Glyphs,
                    "shape" => TextMode::Shape,
                    other => return Err(format!("--text: unknown mode '{other}'")),
                }
            }
            "--without" => {
                let without = &mut args.scene.without;
                for feature in value()?.split(',').map(str::trim).filter(|f| !f.is_empty()) {
                    match feature {
                        "text" => without.text = true,
                        "clip" => without.clip = true,
                        "blend" => without.blend = true,
                        "shadow" => without.shadow = true,
                        "turbulence" => without.turbulence = true,
                        "filters" => without.filters = true,
                        other => return Err(format!("--without: unknown feature '{other}'")),
                    }
                }
            }
            "--stats" => args.stats = true,
            "--tree" => args.tree = true,
            "--tree-size" => args.tree_size = true,
            "--frames" => args.frames = parse_number(&flag, &value()?)?,
            "--passes" => args.passes = parse_number(&flag, &value()?)?,
            "--zooms" => {
                let zooms = value()?
                    .split(',')
                    .map(|z| parse_number(&flag, z.trim()))
                    .collect::<Result<Vec<f32>, _>>()?;
                args.zooms = Some(zooms);
            }
            _ => return Err(format!("unknown option '{flag}'")),
        }
    }
    Ok(args)
}

fn usvg_options() -> usvg::Options<'static> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    options
}

fn positional<'a, const N: usize>(args: &'a Args, names: &str) -> Result<[&'a str; N], String> {
    let found: Vec<&'a str> = args.positional.iter().map(String::as_str).collect();
    found
        .try_into()
        .map_err(|found: Vec<&str>| format!("expected {names}, got {} argument(s)", found.len()))
}

/// The `--stats` line: the scene's counters, what the caller measured, and the frames' peak memory.
fn print_stats(session: &Session, frames: usize, mut timing: JsonLine, baseline: Option<mem::Sample>) {
    let mut json = JsonLine::new();
    json.int("frames", frames as u64);
    session.painter.stats.write_json(&mut json);
    json.int("transient_bytes", session.transient_bytes as u64);
    let (passes, command_buffers) = session.render_passes();
    json.int("render_passes", passes)
        .int("command_buffers", command_buffers as u64);
    timing.int("frames", frames as u64);
    json.object("timing", timing);
    if let (Some(peak), Some(baseline)) = (mem::peaks(), baseline) {
        json.int("baseline_footprint", baseline.footprint)
            .int("baseline_graphics", baseline.graphics)
            .int("peak_footprint", peak.footprint)
            .int("peak_graphics", peak.graphics)
            .int("lifetime_peak_footprint", peak.lifetime);
    }
    eprintln!("{}", json.finish());
}

/// Starts the memory sampler for `--stats` and returns the idle baseline.
fn start_sampling(stats: bool) -> Option<mem::Sample> {
    if !stats {
        return None;
    }
    mem::start();
    let baseline = mem::now();
    mem::reset();
    baseline
}

fn render(args: &Args) -> Result<(), String> {
    let (input, output) = match args.positional.as_slice() {
        [input] if args.tree_size => (input.as_str(), None),
        [input, output] => (input.as_str(), Some(output.as_str())),
        _ => return Err("render expects IN.svg OUT.png".into()),
    };
    let (tree, dropped) = svg::load(input, &usvg_options())?;
    if args.tree {
        svg::dump(tree.root().children(), 0);
    }
    if args.tree_size {
        println!("tree size {} {}", tree.size().width(), tree.size().height());
        return Ok(());
    }
    let output = output.ok_or("render expects IN.svg OUT.png")?;
    let mut session = Session::new(args.scene.clone(), args.canvas)?;
    session.painter.stats.invalid_filter_refs_dropped += dropped;
    let baseline = start_sampling(args.stats);
    let time = session.draw(&tree, args.zoom);
    session.save_png(output)?;
    if args.stats {
        let mut timing = JsonLine::new();
        timing
            .ms("encode_ms", time.encode.as_secs_f64() * 1e3)
            .ms("wall_ms", time.wall.as_secs_f64() * 1e3);
        print_stats(&session, 1, timing, baseline);
    }
    Ok(())
}

fn time(args: &Args) -> Result<(), String> {
    let [input] = positional::<1>(args, "IN.svg")?;
    if args.frames < 2 {
        return Err("--frames must be at least 2".into());
    }
    let (tree, dropped) = svg::load(input, &usvg_options())?;
    let mut session = Session::new(args.scene.clone(), args.canvas)?;
    session.painter.stats.invalid_filter_refs_dropped += dropped;
    let baseline = start_sampling(args.stats);
    let cpu_before = stats::rusage();
    let first = session.draw(&tree, args.zoom);
    let (mut encode, mut wall) = (Vec::new(), Vec::new());
    for _ in 1..args.frames {
        let t = session.draw(&tree, args.zoom);
        encode.push(t.encode.as_secs_f64() * 1e3);
        wall.push(t.wall.as_secs_f64() * 1e3);
    }
    let cpu = stats::rusage();
    let mut json = JsonLine::new();
    json.int("frames", args.frames as u64)
        .ms("first_ms", first.wall.as_secs_f64() * 1e3)
        .object("encode", stats::summary(&encode))
        .object("wall", stats::summary(&wall))
        .ms("user_s", cpu.user_s - cpu_before.user_s)
        .ms("sys_s", cpu.sys_s - cpu_before.sys_s);
    println!("{}", json.finish());
    if args.stats {
        let mut timing = JsonLine::new();
        timing.ms("first_ms", first.wall.as_secs_f64() * 1e3);
        print_stats(&session, args.frames, timing, baseline);
    }
    Ok(())
}

fn ladder(args: &Args) -> Result<(), String> {
    let [input, prefix] = positional::<2>(args, "IN.svg OUTPREFIX")?;
    let zooms = args.zooms.as_deref().ok_or("ladder needs --zooms")?;
    let (tree, dropped) = svg::load(input, &usvg_options())?;
    let mut session = Session::new(args.scene.clone(), args.canvas)?;
    session.painter.stats.invalid_filter_refs_dropped += dropped;
    let baseline = start_sampling(args.stats);
    let (mut encode, mut wall) = (Vec::new(), Vec::new());
    for (step, &zoom) in zooms.iter().enumerate() {
        let t = session.draw(&tree, zoom);
        encode.push(t.encode.as_secs_f64() * 1e3);
        wall.push(t.wall.as_secs_f64() * 1e3);
        session.save_png(&format!("{prefix}.{step:02}.png"))?;
    }
    if args.stats {
        let mut timing = JsonLine::new();
        timing
            .object("encode", stats::summary(&encode))
            .object("wall", stats::summary(&wall));
        print_stats(&session, zooms.len(), timing, baseline);
    }
    Ok(())
}

/// Records per frame the wall and CPU milliseconds of the femtovg part (scene walk, flush and GPU wait; parsing
/// excluded), the transient bytes held at the flush, the process's resident high-water mark, the sampled peak
/// memory and what the frame left held (macOS; empty elsewhere), and the frame's render passes. Growth across passes
/// is what to look for.
fn soak(args: &Args) -> Result<(), String> {
    let [list, out_csv] = positional::<2>(args, "LIST.txt OUT.csv")?;
    let zooms = args.zooms.clone().unwrap_or_else(|| vec![1.0, 2.0, 4.0]);
    let options = usvg_options();
    let files = std::fs::read_to_string(list).map_err(|e| format!("{list}: {e}"))?;
    let mut trees = Vec::new();
    let mut dropped = 0;
    for file in files.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match svg::load(file, &options) {
            Ok((tree, d)) => {
                dropped += d;
                trees.push((file.to_string(), tree));
            }
            Err(e) => eprintln!("svg-harness: skipped {e}"),
        }
    }
    let mut session = Session::new(args.scene.clone(), args.canvas)?;
    session.painter.stats.invalid_filter_refs_dropped += dropped;
    mem::start();
    let mut csv = String::from(
        "pass,file,zoom,wall_ms,cpu_ms,transient_bytes,maxrss,peak_footprint,peak_graphics,\
         footprint_after,graphics_after,passes,command_buffers\n",
    );
    let optional = |value: Option<u64>| value.map(|v| v.to_string()).unwrap_or_default();
    for pass in 0..args.passes {
        for (name, tree) in &trees {
            for &zoom in &zooms {
                mem::reset();
                let before = stats::rusage();
                let t = session.draw(tree, zoom);
                let after = stats::rusage();
                let (peak, held) = (mem::peaks(), mem::now());
                let (passes, command_buffers) = session.render_passes();
                let cpu_ms = (after.user_s + after.sys_s - before.user_s - before.sys_s) * 1e3;
                let short = std::path::Path::new(name)
                    .file_stem()
                    .map_or_else(|| name.clone(), |s| s.to_string_lossy().into_owned());
                csv.push_str(&format!(
                    "{pass},{short},{zoom},{:.3},{cpu_ms:.3},{},{},{},{},{},{},{passes},{command_buffers}\n",
                    t.wall.as_secs_f64() * 1e3,
                    session.transient_bytes,
                    after.maxrss,
                    optional(peak.map(|p| p.footprint)),
                    optional(peak.map(|p| p.graphics)),
                    optional(held.map(|p| p.footprint)),
                    optional(held.map(|p| p.graphics)),
                ));
            }
        }
        eprintln!(
            "pass {pass} done: {} frames so far",
            (pass + 1) * trees.len() * zooms.len()
        );
    }
    std::fs::write(out_csv, csv).map_err(|e| format!("{out_csv}: {e}"))?;
    if args.stats {
        print_stats(&session, args.passes * trees.len() * zooms.len(), JsonLine::new(), None);
    }
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() || raw.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return if raw.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }
    let args = match parse_args(raw) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("svg-harness: {e}\nRun 'svg-harness --help' for usage.");
            return ExitCode::from(2);
        }
    };
    let result = match args.command {
        Command::Render => render(&args),
        Command::Time => time(&args),
        Command::Ladder => ladder(&args),
        Command::Soak => soak(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("svg-harness: {e}");
            ExitCode::FAILURE
        }
    }
}
