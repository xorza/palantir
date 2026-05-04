//! Frame-loop driver around `Ui` that measures heap allocations
//! attributable to one scene's per-frame work.
//!
//! `run_audit` runs `warmup` frames untracked, then drives `audit`
//! frames inside [`with_audit`] so per-thread counters + backtrace
//! capture stay scoped to that window. The counter is per-thread
//! (see `allocator.rs`), so cargo's parallel test runner can't
//! pollute one fixture's window with another's allocations — no
//! global lock needed.

use crate::allocator::{AuditResult, with_audit};
use backtrace::Backtrace;
use palantir::{Display, Ui};
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug)]
pub(crate) struct AllocBudget {
    pub(crate) allocs_per_frame: u64,
}

impl AllocBudget {
    pub(crate) const ZERO: Self = Self {
        allocs_per_frame: 0,
    };
}

pub(crate) fn run_audit<S>(
    name: &str,
    warmup: usize,
    audit: usize,
    budget: AllocBudget,
    mut scene: S,
) where
    S: FnMut(&mut Ui),
{
    assert!(audit > 0, "audit frame count must be > 0");

    let display = Display::from_physical(glam::UVec2::new(800, 600), 1.0);
    let mut ui = Ui::new();

    for _ in 0..warmup {
        ui.begin_frame(display);
        scene(&mut ui);
        let _ = ui.end_frame();
    }

    let mut result = with_audit(|| {
        for _ in 0..audit {
            ui.begin_frame(display);
            scene(&mut ui);
            let _ = ui.end_frame();
        }
    });

    let budget_total = budget.allocs_per_frame * audit as u64;
    let per_frame_allocs = result.allocs as f64 / audit as f64;
    let per_frame_bytes = result.bytes as f64 / audit as f64;
    println!(
        "alloc-audit {name}: {per_frame_allocs:.2} allocs/frame, {per_frame_bytes:.0} B/frame \
         (total {} allocs / {} B over {audit} frames after {warmup} warmup)",
        result.allocs, result.bytes,
    );

    if result.allocs > budget_total {
        dump_traces(&mut result);
        panic!(
            "alloc budget exceeded for `{name}`: {} allocs over {} frames \
             (budget {}/frame = {} total)",
            result.allocs, audit, budget.allocs_per_frame, budget_total,
        );
    }
}

fn dump_traces(result: &mut AuditResult) {
    for (i, bt) in result.traces.iter_mut().enumerate() {
        eprintln!("--- alloc #{i} backtrace ---\n{}", user_frames(bt));
    }
    eprintln!("(set PALANTIR_ALLOC_FULL_BT=1 to disable user-code filtering and see full stacks)",);
}

/// Trim a captured backtrace to just the frames a debug-this reader
/// cares about: `palantir/src/**` (the bug source) plus the entry
/// point inside `tests/alloc/fixtures/**` (the call site). Drops
/// std/runtime, external deps, and the audit machinery itself.
/// Frames are renumbered top-to-bottom so the result reads as a
/// clean call stack from fixture closure down to the allocating
/// call site.
///
/// Resolution is lazy — capture used `Backtrace::new_unresolved`, so
/// symbols/files are only resolved here, on the failure path.
///
/// Set `PALANTIR_ALLOC_FULL_BT=1` to bypass and emit the raw backtrace.
pub(crate) fn user_frames(bt: &mut Backtrace) -> String {
    if std::env::var_os("PALANTIR_ALLOC_FULL_BT").is_some() {
        bt.resolve();
        return format!("{bt:?}");
    }
    bt.resolve();

    let mut out = String::new();
    let mut idx = 0u32;
    let mut seen_fixture_frame = false;
    'outer: for frame in bt.frames() {
        for symbol in frame.symbols() {
            let Some(filename) = symbol.filename() else {
                continue;
            };
            let path = filename.to_string_lossy();
            let Some(rel) = user_relative(&path) else {
                continue;
            };
            let kind = classify(rel);
            let in_fixture = match kind {
                FrameKind::Src => false,
                FrameKind::Fixture => true,
                FrameKind::Other => continue,
            };
            // Stop after the first fixture frame — that's the entry
            // point into the scene closure; further frames are
            // #[test] wrappers that all point at the same file with
            // no extra signal.
            if in_fixture && seen_fixture_frame {
                break 'outer;
            }
            if in_fixture {
                seen_fixture_frame = true;
            }
            let name = symbol
                .name()
                .map(|n| format!("{n:#}"))
                .map(strip_test_crate_prefix)
                .unwrap_or_else(|| String::from("<unknown>"));
            let line = symbol.lineno().unwrap_or(0);
            let col = symbol.colno().unwrap_or(0);
            let _ = writeln!(out, "  {idx:>2}: {name}");
            let _ = writeln!(out, "            at {rel}:{line}:{col}");
            idx += 1;
        }
    }
    if out.is_empty() {
        out.push_str("(no user-code frames matched — full stack:)\n");
        let _ = write!(out, "{bt:?}");
    }
    out
}

#[derive(Clone, Copy)]
enum FrameKind {
    /// Library code under `src/...` — interesting; the bug usually lives here.
    Src,
    /// Fixture entry point under `tests/alloc/fixtures/...` — interesting once
    /// per trace as the call site that triggered the alloc trail.
    Fixture,
    /// Anything else (harness internals under `tests/alloc/`, plus everything
    /// that's not part of the user crate at all) — rejected.
    Other,
}

fn classify(rel: &str) -> FrameKind {
    if rel.starts_with("src/") {
        FrameKind::Src
    } else if rel.starts_with("tests/alloc/fixtures/") {
        FrameKind::Fixture
    } else {
        FrameKind::Other
    }
}

/// Drop the `alloc::` test-binary-crate prefix from a demangled symbol
/// name. The test binary built from `tests/alloc/main.rs` is named
/// `alloc`, so every fixture/harness symbol starts with `alloc::`; that
/// prefix is the same on every line and adds no information.
fn strip_test_crate_prefix(name: String) -> String {
    name.strip_prefix("alloc::")
        .map(String::from)
        .unwrap_or(name)
}

/// Workspace-relative tail of a captured filename, or `None` if the path
/// isn't inside this crate. `backtrace`'s symbol resolver returns absolute
/// paths (`/home/.../palantir/src/widgets/button.rs`), so we strip the
/// crate-root anchor to compare against the user-code prefixes we know.
fn user_relative(path: &str) -> Option<&str> {
    const ANCHOR: &str = "/palantir/";
    let idx = path.rfind(ANCHOR)?;
    Some(&path[idx + ANCHOR.len()..])
}
