//! Frame-loop driver around `Ui` that measures heap allocations
//! attributable to one scene's per-frame work.
//!
//! `run_audit` runs `warmup` frames untracked, snapshots the
//! per-thread counter, runs `audit` frames, and asserts the delta is
//! within `budget`. The counter is per-thread (see `allocator.rs`),
//! so cargo's parallel test runner can't pollute one fixture's
//! window with another's allocations — no global lock needed.
//! With `RUST_BACKTRACE=1`, every audit-window alloc is captured
//! and dumped on budget failure.

use crate::allocator::{Backtrace, delta, set_in_audit, snapshot, take_traces};
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
    let before = snapshot();
    set_in_audit(true);
    for _ in 0..audit {
        ui.begin_frame(display);
        scene(&mut ui);
        let _ = ui.end_frame();
    }
    set_in_audit(false);
    let measured = delta(before);
    let mut traces = take_traces();

    let budget_total = budget.allocs_per_frame * audit as u64;
    let per_frame_allocs = measured.allocs as f64 / audit as f64;
    let per_frame_bytes = measured.bytes as f64 / audit as f64;
    println!(
        "alloc-audit {name}: {per_frame_allocs:.2} allocs/frame, {per_frame_bytes:.0} B/frame \
         (total {} allocs / {} B over {audit} frames after {warmup} warmup)",
        measured.allocs, measured.bytes,
    );

    if measured.allocs > budget_total {
        for (i, bt) in traces.iter_mut().enumerate() {
            eprintln!("--- alloc #{i} backtrace ---\n{}", user_frames(bt));
        }
        eprintln!(
            "(set PALANTIR_ALLOC_FULL_BT=1 to disable user-code filtering and see full stacks)",
        );
        panic!(
            "alloc budget exceeded for `{name}`: {} allocs over {} frames \
             (budget {}/frame = {} total, {:.2} actual/frame)",
            measured.allocs, audit, budget.allocs_per_frame, budget_total, per_frame_allocs,
        );
    }
}

/// Trim a captured backtrace to just the frames a debug-this reader
/// cares about: `palantir/src/**` (the bug source) and the fixture
/// closure (the call site). Drops std/runtime, external deps, and the
/// audit machinery itself (allocator/harness/main). Frames are
/// renumbered top-to-bottom so the result reads as a clean call stack
/// from fixture closure down to the allocating call site.
///
/// Resolution is lazy — capture used `Backtrace::new_unresolved`, so
/// symbols/files are only resolved here, on the failure path.
///
/// Set `PALANTIR_ALLOC_FULL_BT=1` to bypass and emit the raw backtrace.
fn user_frames(bt: &mut Backtrace) -> String {
    if std::env::var_os("PALANTIR_ALLOC_FULL_BT").is_some() {
        bt.resolve();
        return format!("{bt:?}");
    }
    bt.resolve();

    let mut out = String::new();
    let mut idx = 0u32;
    let mut seen_test_frame = false;
    'outer: for frame in bt.frames() {
        for symbol in frame.symbols() {
            let Some(filename) = symbol.filename() else {
                continue;
            };
            let path = filename.to_string_lossy();
            if !is_user_path(&path) {
                continue;
            }
            let rel = user_relative(&path).unwrap_or(&path);
            // Stop after the first `tests/` frame — that's the entry point
            // into the fixture closure; further frames are #[test] wrappers
            // (the test fn body, the outer test-macro closure) which all
            // point at the same file with no extra signal.
            let in_test = rel.starts_with("tests/");
            if in_test && seen_test_frame {
                break 'outer;
            }
            if in_test {
                seen_test_frame = true;
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

fn is_user_path(path: &str) -> bool {
    // Allow `src/...` and `tests/...` in this crate; reject the audit
    // plumbing inside `tests/alloc/`. Anything outside the crate
    // (rustc, rustup, `.cargo/registry`, hashbrown, etc.) is rejected
    // by the anchor check.
    let Some(rel) = user_relative(path) else {
        return false;
    };
    let user = rel.starts_with("src/") || rel.starts_with("tests/");
    let plumbing = rel.starts_with("tests/alloc/allocator.rs")
        || rel.starts_with("tests/alloc/harness.rs")
        || rel.starts_with("tests/alloc/main.rs");
    user && !plumbing
}
