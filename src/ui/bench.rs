//! Per-frame aggregate benchmark: two benches selected by the [`Arms`] the runner
//! hands [`bench`](fn@bench) (`cpu` / `gpu` / `both`).
//!
//! - **`bench_cpu`** (`frame/*_cpu`): the CPU pipeline on a bare `Ui` and
//!   standalone `Frontend` with no wgpu device, from record through encode +
//!   compose. Going through the offscreen driver plus a poll charges every iter
//!   driver time (~20% on `cached_cpu`, ~50% on `resizing_cpu`) that swamps the
//!   cost measured.
//! - **`bench_gpu`** (`frame/*_gpu`): `OffscreenHost::frame` against an offscreen
//!   texture with `PollType::Wait`, on the desktop's present strategy (see
//!   `bench_host`). The per-frame `write_stats` dump lives here.
//!
//! `--arms cpu` executes no GPU code, so a `perf` / `samply` capture is clean.
//!
//! Arms, in both benches:
//!
//! - **`cached_*`**: fixed viewport, MeasureCache hits, damage `Skip`.
//! - **`partial_*`**: one fixture counter changes per iter, so damage is one
//!   small `Partial` rect.
//! - **`resizing_*`**: rotates surface sizes so `available_q` busts the measure
//!   cache each iter.
//! - **`scrolling_*`**: shifts a `Panel::transform` so only the cascade walk sees
//!   change.
//! - **`alternating_*`**: a scroll step (full repaint) then a counter tick (small
//!   partial); measures what a full frame costs the partial after it.
//!
//! Each arm's criterion `time:` estimate is prepended to
//! `benches/results/<machine>.txt`; `--machine` overrides the `hostname -s` name
//! and `--note` captions the row. `--size <w>x<h>` and `--scale <dpr>` override
//! the surface. The fixture is taller than the 1440p default, so CPU arms process
//! the whole tree while paint and GPU arms see only the screen. All four arrive
//! in [`Run::fixture`].
//!
//! The workload lives in [`crate::internals::frame_fixture`], shared with the
//! allocation gates in `tests/alloc/gates/` and the showcase's `frame bench` page.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::{Arms, Fixture, Run};
use crate::gpu::bench_gpu::{BenchGpu, BenchTarget, Timing};
use crate::gpu::resource::texture_region::counters::WriteStats;
use crate::host::offscreen::OffscreenHost;
use crate::internals::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use crate::internals::harness::UiHarness;
use crate::internals::harness::frontend_harness::FrontendHarness;
use crate::internals::record_app::RecordApp;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::slice;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The window clear both halves render over.
const WINDOW_CLEAR: RgbaF32 = RgbaF32::BLACK;
// Proportioned against `BENCH_SURFACE` and rescaled by `--size`; multiples of 16
// keep a resized surface tile-aligned.
const RESIZE_POOL: &[glam::UVec2] = &[
    glam::UVec2::new(2144, 1344),
    glam::UVec2::new(2560, 1440),
    glam::UVec2::new(2352, 1392),
    glam::UVec2::new(2768, 1488),
];

#[derive(Clone, Debug)]
struct Surface {
    size: glam::UVec2,
    scale: f32,
    pool: Vec<glam::UVec2>,
}

impl Surface {
    fn new(fixture: Fixture<'_>) -> Self {
        let size = fixture.size.unwrap_or(BENCH_SURFACE);
        let ratio = size.as_vec2() / BENCH_SURFACE.as_vec2();
        Surface {
            size,
            scale: fixture.scale.unwrap_or(BENCH_DPR),
            pool: RESIZE_POOL
                .iter()
                .map(|s| (s.as_vec2() * ratio).round().as_uvec2())
                .collect(),
        }
    }
}

fn gpu() -> &'static BenchGpu {
    static ANNOUNCED: OnceLock<()> = OnceLock::new();

    let gpu = BenchGpu::shared(Timing::PassOnly);
    ANNOUNCED.get_or_init(|| {
        eprintln!("[frame] {}", gpu.summary());
    });
    gpu
}

/// A host on the desktop's present strategy (`DirectAdaptive`, as the winit host
/// runs). The default would measure the screenshot path, which copies the
/// backbuffer out on skip and full frames.
fn bench_host(g: &BenchGpu, collect_gpu_stats: bool) -> OffscreenHost {
    let mut host = g
        .offscreen_builder()
        .collect_gpu_stats(collect_gpu_stats)
        .retained_target(true)
        .build();
    host.ui().theme_mut().window_clear = WINDOW_CLEAR;
    host
}

/// Deviceless CPU-pipeline harness: a bare `Ui` plus a standalone `Frontend`,
/// stopping before any GPU submit. Time comes from a real `Instant` as in
/// `WindowDriver::cpu_frame`; a frozen clock could classify frames `PaintOnly` and
/// skip the record closure.
#[derive(Debug)]
struct CpuHarness {
    frontend: FrontendHarness,
    start: Instant,
}

impl CpuHarness {
    fn new(surface: &Surface) -> Self {
        let mut harness = UiHarness::with_text(surface.size).scale(surface.scale);
        harness.ui.theme_mut().window_clear = WINDOW_CLEAR;
        Self {
            frontend: FrontendHarness::new(harness),
            start: Instant::now(),
        }
    }

    /// Drive one full CPU frame and ack the present, so `cached` settles into `Skip`.
    fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FramePaint {
        self.frontend.harness.at(self.start.elapsed());
        self.frontend.frame(record).paint()
    }
}

/// What changes between two frames of one arm. One list for both halves, so a
/// CPU arm and its GPU twin always measure the same frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Alternating,
    Cached,
    Partial,
    Resizing,
    Scrolling,
}

impl Arm {
    /// Sorted by label, the order criterion and the results file list them.
    const ALL: [Self; 5] = [
        Self::Alternating,
        Self::Cached,
        Self::Partial,
        Self::Resizing,
        Self::Scrolling,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Alternating => "alternating",
            Self::Cached => "cached",
            Self::Partial => "partial",
            Self::Resizing => "resizing",
            Self::Scrolling => "scrolling",
        }
    }

    /// The paint a settled frame `n` plans, cycling by `n`: pinned before the arm
    /// is measured, since a regression to another plan still prints a number.
    const fn paints(self) -> &'static [FramePaint] {
        match self {
            Self::Alternating => &[FramePaint::Full, FramePaint::Partial],
            Self::Cached => &[FramePaint::Skip],
            Self::Partial => &[FramePaint::Partial],
            Self::Resizing | Self::Scrolling => &[FramePaint::Full],
        }
    }

    /// The surfaces the arm renders at, frame `n` at `[n % len]`.
    fn sizes(self, surface: &Surface) -> &[glam::UVec2] {
        match self {
            Self::Resizing => &surface.pool,
            Self::Alternating | Self::Cached | Self::Partial | Self::Scrolling => {
                slice::from_ref(&surface.size)
            }
        }
    }

    /// Set the fixture to frame `n`'s state.
    fn advance(self, state: &mut FrameFixture, n: u32) {
        match self {
            Self::Cached | Self::Resizing => {}
            Self::Partial => state.tick = state.tick.wrapping_add(1),
            Self::Scrolling => scroll(state),
            // A scroll step on even frames (full repaint), a counter tick on odd ones (small rect).
            Self::Alternating if n.is_multiple_of(2) => scroll(state),
            Self::Alternating => state.tick = state.tick.wrapping_add(1),
        }
    }
}

/// Shift the fixture's transform, so only the cascade walk sees change.
fn scroll(state: &mut FrameFixture) {
    state.scroll_offset.x = (state.scroll_offset.x + 1.5) % 256.0;
    state.scroll_offset.y = (state.scroll_offset.y + 0.7) % 256.0;
}

/// Frames each arm runs before its paint is pinned: even, so the alternating arm
/// starts its check on a scroll step.
const SETTLE_FRAMES: u32 = 4;

/// Run `frame` through [`SETTLE_FRAMES`], pin one cycle of [`Arm::paints`],
/// then measure it as `<arm>_<half>`.
fn measure_arm(
    group: &mut BenchmarkGroup<'_, WallTime>,
    arm: Arm,
    half: &str,
    mut frame: impl FnMut(&mut FrameFixture, u32) -> FramePaint,
) {
    let mut state = FrameFixture::default();
    let mut n = 0u32;
    let mut step = || {
        arm.advance(&mut state, n);
        let paint = frame(&mut state, n);
        n = n.wrapping_add(1);
        paint
    };
    for _ in 0..SETTLE_FRAMES {
        step();
    }
    for &want in arm.paints() {
        assert_eq!(
            step(),
            want,
            "the {}_{half} arm's frames plan another paint",
            arm.label()
        );
    }
    group.bench_function(format!("{}_{half}", arm.label()), |b| {
        b.iter(&mut step);
    });
}

fn bench_cpu(c: &mut Criterion, run: Run<'_>, surface: &Surface) {
    if !run.arms.includes_cpu() {
        return;
    }
    let mut group = run.group(c);
    for arm in Arm::ALL {
        let mut h = CpuHarness::new(surface);
        let sizes = arm.sizes(surface);
        measure_arm(&mut group, arm, "cpu", |state, n| {
            if arm == Arm::Resizing {
                h.frontend.harness.resize(sizes[n as usize % sizes.len()]);
            }
            h.frame(|ui| state.render(BENCH_SCALE, ui))
        });
    }
    group.finish();
}

fn bench_gpu(c: &mut Criterion, run: Run<'_>, surface: &Surface) {
    if !run.arms.includes_gpu() {
        return;
    }
    let g = gpu();
    report_write_stats(surface);
    let mut group = run.group(c);
    for arm in Arm::ALL {
        let targets = targets(arm, surface, "palantir.frame_bench");
        let mut host = bench_host(g, false);
        measure_arm(&mut group, arm, "gpu", |state, n| {
            let target = &targets[n as usize % targets.len()];
            let paint = gpu_frame(&mut host, target, surface.scale, state);
            g.wait();
            paint
        });
    }
    group.finish();
}

/// One target per surface the arm renders at.
fn targets(arm: Arm, surface: &Surface, label: &str) -> Vec<BenchTarget> {
    arm.sizes(surface)
        .iter()
        .enumerate()
        .map(|(i, size)| gpu().target(&format!("{label}.{}.{i}", arm.label()), *size))
        .collect()
}

fn gpu_frame(
    host: &mut OffscreenHost,
    target: &BenchTarget,
    system_scale: f32,
    state: &mut FrameFixture,
) -> FramePaint {
    let mut app = RecordApp::new(|ui| state.render(BENCH_SCALE, ui));
    host.frame(target.as_target(), system_scale, &mut app)
        .paint()
}

/// Per-frame `queue.write_*` counts and GPU main-pass time per arm, frames 0..=5.
/// The pass readout lags a frame, so frame 0's column is omitted.
fn report_write_stats(surface: &Surface) {
    let g = gpu();
    for arm in Arm::ALL {
        let targets = targets(arm, surface, "write_stats");
        let mut host = bench_host(g, true);
        let mut state = FrameFixture::default();
        eprintln!("[write_stats] {}:", arm.label());
        for frame in 0..6 {
            arm.advance(&mut state, frame);
            let _ = WriteStats::take();
            let target = &targets[frame as usize % targets.len()];
            gpu_frame(&mut host, target, surface.scale, &mut state);
            g.wait();
            let s = WriteStats::take();
            // The pass-time readout lags one frame (`map_async` fires off the next poll); one
            // extra Poll drains this frame's resolve.
            g.poll();
            let stats = host.gpu_pass_stats();
            let gpu = stats.last_pass().map_or_else(
                || "  n/a   ".into(),
                |d| format!("{:>5.2} ms", d.as_secs_f64() * 1e3),
            );
            // TIMESTAMP_QUERY_INSIDE_ENCODERS, on a frame that copied out.
            let copy_out = stats.last_copy_out().map_or_else(String::new, |d| {
                format!("  copy_out: {:.2} ms", d.as_secs_f64() * 1e3)
            });
            eprintln!(
                "  frame {frame}  texture: {:>2} calls, {:>9} B   gpu: {gpu}{copy_out}",
                s.texture_calls, s.texture_bytes,
            );
            if let Some(p) = stats.last_pipeline_stats() {
                eprintln!(
                    "           pipeline: vs={} clip_in={} clip_out={} fs={}",
                    p.vertex_shader_invocations,
                    p.clipper_invocations,
                    p.clipper_primitives_out,
                    p.fragment_shader_invocations,
                );
            }
        }
    }
}

/// Arm ids criterion runs for a mode, interleaved cpu/gpu per arm, built from
/// the namespace the benches register under.
fn arm_names(run: Run<'_>) -> Vec<String> {
    let group = run.group_name();
    let mut v = Vec::with_capacity(Arm::ALL.len() * 2);
    for arm in Arm::ALL {
        if run.arms.includes_cpu() {
            v.push(format!("{group}/{}_cpu", arm.label()));
        }
        if run.arms.includes_gpu() {
            v.push(format!("{group}/{}_gpu", arm.label()));
        }
    }
    v
}

/// Results finalizer: runs last, only when the run records. Prepends each arm's
/// `[lower point upper]` from `target/criterion/<group>/<arm>/new/estimates.json`
/// to the per-machine `.txt`, newest on top.
fn prepend_machine_results(run: Run<'_>) {
    let machine = machine_label(run.fixture.machine);
    let mut block = String::new();
    let mode_tag = match run.arms {
        Arms::Cpu => "cpu",
        Arms::Gpu => "gpu",
        Arms::Both => "both",
    };
    writeln!(
        block,
        "=== {} — [{}] {} ===",
        now_label(),
        mode_tag,
        bench_annotation(run.fixture.note)
    )
    .unwrap();
    for name in arm_names(run) {
        let name = name.as_str();
        let row = match read_criterion_estimate(name) {
            Some(e) => format!("{name:<22} time: {}\n", fmt_estimate(e)),
            None => format!("{name:<22} time: (criterion estimates not found)\n"),
        };
        block.push_str(&row);
    }
    block.push('\n');

    prepend_block(Path::new("benches/results"), &machine, &block);
}

/// Put `block` at the top of `<dir>/<machine>.txt`, keeping the rest. Best-effort:
/// losing a row must not fail a finished bench. An unreadable existing file stops
/// the write (only `NotFound` means no history), or the rename would replace every
/// past row; content is read as bytes since non-UTF-8 isn't this run's to discard.
/// Split out so a test can use its own directory.
fn prepend_block(dir: &Path, machine: &str, block: &str) {
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!("[machine-results] create {}: {e}", dir.display());
        return;
    }
    let path = dir.join(format!("{machine}.txt"));
    let prior = match fs::read(&path) {
        Ok(prior) => prior,
        Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            eprintln!(
                "[machine-results] read {}: {e} — history kept, this row dropped",
                path.display(),
            );
            return;
        }
    };
    // Tempfile then rename, so an interrupted bench leaves no half-written file.
    let tmp_path = path.with_extension("txt.tmp");
    let mut f = match OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[machine-results] open {}: {e}", tmp_path.display());
            return;
        }
    };
    if let Err(e) = f
        .write_all(block.as_bytes())
        .and_then(|()| f.write_all(&prior))
    {
        eprintln!("[machine-results] write {}: {e}", tmp_path.display());
        return;
    }
    drop(f);
    if let Err(e) = fs::rename(&tmp_path, &path) {
        eprintln!(
            "[machine-results] rename {} -> {}: {e}",
            tmp_path.display(),
            path.display()
        );
        return;
    }
    eprintln!("[machine-results] prepended to {}", path.display());
}

#[derive(Debug, Clone, Copy)]
struct Estimate {
    lo_ns: f64,
    mid_ns: f64,
    hi_ns: f64,
}

/// Locate criterion's output root: the `target/` the bench binary lives in. A
/// parent workspace building palantir as a path dependency uses its own `target/`,
/// and a CWD walk-up could find a stale `palantir/target/criterion`.
fn criterion_root() -> PathBuf {
    if let Ok(t) = env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(t).join("criterion");
    }
    // The first ancestor named "target", deepest-first, whatever the profile dir is called.
    if let Ok(exe) = env::current_exe()
        && let Some(target) = exe
            .ancestors()
            .find(|a| a.file_name() == Some("target".as_ref()))
    {
        return target.join("criterion");
    }
    PathBuf::from("target").join("criterion")
}

/// Extract the estimate criterion's `time:` line reports: the **slope**, else the
/// **mean** when `"slope":null`. Single-line JSON, so slice instead of pulling in
/// serde_json.
fn read_criterion_estimate(name: &str) -> Option<Estimate> {
    let s = fs::read_to_string(estimates_path(&criterion_root(), name)).ok()?;
    estimate_from_block(&s, "\"slope\":").or_else(|| estimate_from_block(&s, "\"mean\":"))
}

/// Where criterion filed `name`'s estimate: one directory per `/` component.
fn estimates_path(root: &Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(root.to_path_buf(), |dir, part| dir.join(part))
        .join("new/estimates.json")
}

/// Read `{lower_bound, point_estimate, upper_bound}` from the `key` block; `None`
/// if absent or null, or the scan would run into the next block.
fn estimate_from_block(s: &str, key: &str) -> Option<Estimate> {
    let after = &s[s.find(key)? + key.len()..];
    if after.trim_start().starts_with("null") {
        return None;
    }
    Some(Estimate {
        lo_ns: extract_json_number(after, "\"lower_bound\":")?,
        mid_ns: extract_json_number(after, "\"point_estimate\":")?,
        hi_ns: extract_json_number(after, "\"upper_bound\":")?,
    })
}

fn extract_json_number(s: &str, key: &str) -> Option<f64> {
    let i = s.find(key)? + key.len();
    let rest = &s[i..];
    let end = rest
        .find(|c: char| {
            !c.is_ascii_digit() && c != '.' && c != '-' && c != '+' && c != 'e' && c != 'E'
        })
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Render µs or ms with two decimals, unit per value.
fn fmt_estimate(e: Estimate) -> String {
    fn one(ns: f64) -> String {
        let us = ns / 1_000.0;
        if us < 1000.0 {
            format!("{us:7.2} µs")
        } else {
            format!("{:7.3} ms", us / 1000.0)
        }
    }
    format!("[{} {} {}]", one(e.lo_ns), one(e.mid_ns), one(e.hi_ns))
}

/// `--machine` overrides the hostname label; sanitized to lowercase alnum + `-_`,
/// first dotted component, falling back to `gethostname`, then `unknown`.
fn machine_label(machine: Option<&str>) -> String {
    fn sanitize(raw: &str) -> String {
        raw.trim()
            .split('.')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_lowercase()
    }
    if let Some(given) = machine {
        let n = sanitize(given);
        if !n.is_empty() {
            return n;
        }
    }
    let raw = gethostname::gethostname();
    let n = sanitize(&raw.to_string_lossy());
    if n.is_empty() { "unknown".into() } else { n }
}

/// Required context tag from `--note`; the bench refuses to run without one.
fn bench_annotation(note: Option<&str>) -> &str {
    match note.map(str::trim) {
        Some(s) if !s.is_empty() => s,
        _ => panic!(
            "frame bench requires a note; e.g. cargo bench --bench criterion \
             -- -d frame --note 'after staging-belt rework'",
        ),
    }
}

fn now_label() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%d %H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown-time".into(), |s| s.trim().to_owned())
}

// Longer window than criterion's default: GPU arms vary ±15-25% across runs.
pub(crate) fn config() -> Criterion {
    Criterion::default()
        .measurement_time(Duration::from_secs(12))
        .warm_up_time(Duration::from_secs(3))
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    // Test and profile modes write no estimate, so the note is moot; fail fast.
    if run.recording {
        let _ = bench_annotation(run.fixture.note);
    }
    let surface = Surface::new(run.fixture);
    bench_cpu(c, run, &surface);
    bench_gpu(c, run, &surface);
    if run.recording {
        prepend_machine_results(run);
    }
}

#[cfg(test)]
mod tests {
    use crate::bench::Fixture;
    use crate::internals::frame_fixture::{BENCH_DPR, BENCH_SURFACE};
    use crate::ui::bench::{RESIZE_POOL, Surface, estimates_path, prepend_block};
    use std::env;
    use std::fs;
    use std::path;
    use std::process;

    #[test]
    fn a_row_creates_the_missing_results_dir_and_lands_on_top() {
        let root = env::temp_dir().join(format!("palantir-bench-results-{}", process::id()));
        let dir = root.join("results");
        let _ = fs::remove_dir_all(&root);
        assert!(!dir.exists(), "the case under test is an absent dir");

        prepend_block(&dir, "rig", "older\n");
        prepend_block(&dir, "rig", "newer\n");

        let path = dir.join("rig.txt");
        assert_eq!(fs::read_to_string(&path).unwrap(), "newer\nolder\n");
        assert!(
            !dir.join("rig.txt.tmp").exists(),
            "the rename must leave no tempfile behind",
        );
        prepend_block(&dir, "other", "elsewhere\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "newer\nolder\n");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_unreadable_history_is_kept_rather_than_replaced() {
        let root = env::temp_dir().join(format!("palantir-bench-unreadable-{}", process::id()));
        let dir = root.join("results");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();

        let prior: &[u8] = b"\xff\xfe older\n";
        let path = dir.join("rig.txt");
        fs::write(&path, prior).unwrap();
        prepend_block(&dir, "rig", "newer\n");
        let mut want = b"newer\n".to_vec();
        want.extend_from_slice(prior);
        assert_eq!(
            fs::read(&path).unwrap(),
            want,
            "the row goes on top of bytes this run cannot decode",
        );

        let blocked = dir.join("other.txt");
        fs::create_dir(&blocked).unwrap();
        prepend_block(&dir, "other", "newer\n");
        assert!(blocked.is_dir(), "an unreadable destination is left alone");
        assert!(
            !dir.join("other.txt.tmp").exists(),
            "and the write never starts, so no tempfile is left behind",
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_given_size_scales_the_resize_pool_by_the_same_ratio() {
        let d = Surface::new(Fixture::default());
        assert_eq!(d.size, BENCH_SURFACE);
        assert_eq!(d.scale, BENCH_DPR);
        assert_eq!(d.pool, RESIZE_POOL, "unset size leaves the pool alone");

        let half = Surface::new(Fixture {
            size: Some(glam::UVec2::new(1280, 720)),
            scale: Some(1.0),
            ..Fixture::default()
        });
        assert_eq!(half.size, glam::UVec2::new(1280, 720));
        assert_eq!(half.scale, 1.0);
        assert_eq!(
            half.pool,
            [
                glam::UVec2::new(1072, 672),
                glam::UVec2::new(1280, 720),
                glam::UVec2::new(1176, 696),
                glam::UVec2::new(1384, 744),
            ],
        );

        let odd = Surface::new(Fixture {
            size: Some(glam::UVec2::new(2100, 1440)),
            ..Fixture::default()
        });
        assert_eq!(odd.pool[0].x, 1759);
        assert_eq!(odd.scale, BENCH_DPR, "unset scale keeps the default");
    }

    #[test]
    fn estimates_path_nests_the_group_and_arm() {
        let root = path::Path::new("/t/criterion");
        assert_eq!(
            estimates_path(root, "frame/cached_gpu"),
            root.join("frame")
                .join("cached_gpu")
                .join("new/estimates.json"),
        );
        assert_eq!(
            estimates_path(root, "solo"),
            root.join("solo").join("new/estimates.json"),
            "an id with no group is one directory deep",
        );
    }
}
