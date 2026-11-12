//! What a run drew and what it cost: the scene's counters, frame timings, CPU time and peak memory.

use std::fmt::Write;

/// What the scene walk did, summed over every frame of a run.
#[derive(Clone, Debug, Default)]
pub struct Counters {
    pub layers: usize,
    /// Layers the canvas could not allocate, drawn straight into their parent without their effects.
    pub layers_passed_through: usize,
    pub clip_coverage_layers: usize,
    /// Groups left out because their filter replaces the source graphic and cannot be run.
    pub filters_skipped: usize,
    pub invalid_filter_refs_dropped: usize,
    pub text_femtovg: usize,
    pub text_outlines: usize,
    pub text_errors: usize,
    pub fills: usize,
    pub strokes: usize,
}

/// One line of JSON, built field by field.
pub struct JsonLine(String);

impl JsonLine {
    pub fn new() -> Self {
        Self(String::from("{"))
    }

    fn key(&mut self, key: &str) {
        if self.0.len() > 1 {
            self.0.push(',');
        }
        let _ = write!(self.0, "\"{key}\":");
    }

    pub fn int(&mut self, key: &str, value: impl Into<u64>) -> &mut Self {
        self.key(key);
        let _ = write!(self.0, "{}", value.into());
        self
    }

    pub fn ms(&mut self, key: &str, value: f64) -> &mut Self {
        self.key(key);
        let _ = write!(self.0, "{value:.3}");
        self
    }

    pub fn object(&mut self, key: &str, value: JsonLine) -> &mut Self {
        self.key(key);
        self.0.push_str(&value.finish());
        self
    }

    pub fn finish(mut self) -> String {
        self.0.push('}');
        self.0
    }
}

impl Counters {
    pub fn write_json(&self, json: &mut JsonLine) {
        let fields = [
            ("layers", self.layers),
            ("layers_passed_through", self.layers_passed_through),
            ("clip_coverage_layers", self.clip_coverage_layers),
            ("filters_skipped", self.filters_skipped),
            ("invalid_filter_refs_dropped", self.invalid_filter_refs_dropped),
            ("text_femtovg", self.text_femtovg),
            ("text_outlines", self.text_outlines),
            ("text_errors", self.text_errors),
            ("fills", self.fills),
            ("strokes", self.strokes),
        ];
        for (key, value) in fields {
            json.int(key, value as u64);
        }
    }
}

/// Median, 95th percentile (nearest rank) and minimum of `samples`, in its unit.
pub fn summary(samples: &[f64]) -> JsonLine {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut json = JsonLine::new();
    if let Some(first) = sorted.first() {
        let rank = |q: f64| sorted[((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1];
        let median = if sorted.len() % 2 == 1 {
            sorted[sorted.len() / 2]
        } else {
            (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
        };
        json.ms("median_ms", median)
            .ms("p95_ms", rank(0.95))
            .ms("min_ms", *first);
    }
    json
}

/// The process's CPU time so far and its resident high-water mark (bytes on macOS, KiB on Linux).
#[derive(Clone, Copy, Debug, Default)]
pub struct Rusage {
    pub user_s: f64,
    pub sys_s: f64,
    pub maxrss: u64,
}

#[cfg(unix)]
pub fn rusage() -> Rusage {
    // `struct rusage` on 64-bit Linux and macOS: two timevals, then 14 longs. macOS's `tv_usec` is an int padded
    // to 8 bytes, which reads correctly as the low half of a little-endian long.
    #[repr(C)]
    struct Timeval {
        tv_sec: i64,
        tv_usec: i64,
    }
    #[repr(C)]
    struct RawRusage {
        ru_utime: Timeval,
        ru_stime: Timeval,
        ru_maxrss: i64,
        rest: [i64; 13],
    }
    extern "C" {
        fn getrusage(who: i32, usage: *mut RawRusage) -> i32;
    }
    const RUSAGE_SELF: i32 = 0;
    let mut r = RawRusage {
        ru_utime: Timeval { tv_sec: 0, tv_usec: 0 },
        ru_stime: Timeval { tv_sec: 0, tv_usec: 0 },
        ru_maxrss: 0,
        rest: [0; 13],
    };
    // SAFETY: `r` matches the platform's `struct rusage` in size and layout.
    if unsafe { getrusage(RUSAGE_SELF, &mut r) } != 0 {
        return Rusage::default();
    }
    let seconds = |t: &Timeval| t.tv_sec as f64 + (t.tv_usec & 0xffff_ffff) as f64 / 1e6;
    Rusage {
        user_s: seconds(&r.ru_utime),
        sys_s: seconds(&r.ru_stime),
        maxrss: r.ru_maxrss as u64,
    }
}

#[cfg(not(unix))]
pub fn rusage() -> Rusage {
    Rusage::default()
}

/// Peak memory while frames render: a thread samples the task's ledgers (`task_info(TASK_VM_INFO)`) every 200 µs
/// and keeps the maxima since the last `reset`. `footprint` is what jetsam and Activity Monitor count, CPU and GPU
/// together; `graphics` is the part tagged graphics, on Apple silicon Metal's textures, buffers and command memory.
/// macOS only: elsewhere `sample` is `None` and nothing is sampled.
pub mod mem {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Sample {
        pub footprint: u64,
        pub graphics: u64,
        /// The kernel's own lifetime maximum of the footprint.
        pub lifetime: u64,
    }

    #[cfg(target_os = "macos")]
    mod imp {
        use super::Sample;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

        static FOOTPRINT: AtomicU64 = AtomicU64::new(0);
        static GRAPHICS: AtomicU64 = AtomicU64::new(0);
        static STARTED: AtomicBool = AtomicBool::new(false);

        /// `struct task_vm_info` of `<mach/task_info.h>`, packed to 4 bytes: 93 words.
        fn ledgers() -> Option<[u32; 93]> {
            extern "C" {
                static mach_task_self_: u32;
                fn task_info(task: u32, flavor: u32, info: *mut u32, count: *mut u32) -> i32;
            }
            const TASK_VM_INFO: u32 = 22;
            let mut words = [0u32; 93];
            let mut count = words.len() as u32;
            // SAFETY: `words` holds `count` words, the size of `task_vm_info`.
            (unsafe { task_info(mach_task_self_, TASK_VM_INFO, words.as_mut_ptr(), &mut count) } == 0).then_some(words)
        }

        fn field(words: &[u32; 93], byte_offset: usize) -> u64 {
            words[byte_offset / 4] as u64 | (words[byte_offset / 4 + 1] as u64) << 32
        }

        pub fn now() -> Option<Sample> {
            const PHYS_FOOTPRINT: usize = 144;
            const LEDGER_PHYS_FOOTPRINT_PEAK: usize = 168;
            const LEDGER_TAG_GRAPHICS_FOOTPRINT: usize = 272;
            const LEDGER_TAG_GRAPHICS_FOOTPRINT_COMPRESSED: usize = 280;
            let words = ledgers()?;
            Some(Sample {
                footprint: field(&words, PHYS_FOOTPRINT),
                graphics: field(&words, LEDGER_TAG_GRAPHICS_FOOTPRINT)
                    + field(&words, LEDGER_TAG_GRAPHICS_FOOTPRINT_COMPRESSED),
                lifetime: field(&words, LEDGER_PHYS_FOOTPRINT_PEAK),
            })
        }

        pub fn start() {
            if STARTED.swap(true, Relaxed) {
                return;
            }
            std::thread::spawn(|| loop {
                if let Some(s) = now() {
                    FOOTPRINT.fetch_max(s.footprint, Relaxed);
                    GRAPHICS.fetch_max(s.graphics, Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_micros(200));
            });
        }

        pub fn reset() {
            let s = now().unwrap_or_default();
            FOOTPRINT.store(s.footprint, Relaxed);
            GRAPHICS.store(s.graphics, Relaxed);
        }

        pub fn peaks() -> Option<Sample> {
            let s = now()?;
            Some(Sample {
                footprint: FOOTPRINT.load(Relaxed).max(s.footprint),
                graphics: GRAPHICS.load(Relaxed).max(s.graphics),
                lifetime: s.lifetime,
            })
        }
    }

    #[cfg(not(target_os = "macos"))]
    mod imp {
        use super::Sample;

        pub fn now() -> Option<Sample> {
            None
        }
        pub fn start() {}
        pub fn reset() {}
        pub fn peaks() -> Option<Sample> {
            None
        }
    }

    /// The ledgers now.
    pub use imp::now;
    /// The maxima since the last `reset`.
    pub use imp::peaks;
    /// Forgets the maxima: the next `peaks` covers what happens from here.
    pub use imp::reset;
    /// Starts the sampler thread, once.
    pub use imp::start;
}
