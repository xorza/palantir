//! Backtrace filter + pretty-printer for audit failures. Resolves
//! captured `backtrace::Backtrace`s lazily (capture used
//! `new_unresolved`) and renders only the frames a debug-this reader
//! cares about: palantir `src/...` (where the bug usually lives) and
//! the entry point inside `tests/alloc/fixtures/...` (the call site).
//! Std/runtime, external deps, and the audit machinery itself are
//! dropped. Demangled names are stripped of the `alloc::` test-binary
//! prefix and the `::h<hash>` suffix.

use backtrace::Backtrace;
use std::fmt::Write as _;

/// Render `bt` as a tight call stack from fixture closure down to the
/// allocating call site. With `PALANTIR_ALLOC_FULL_BT=1`, bypass the
/// filter and dump the raw resolved backtrace instead.
pub(crate) fn user_frames(bt: &mut Backtrace) -> String {
    bt.resolve();
    if std::env::var_os("PALANTIR_ALLOC_FULL_BT").is_some() {
        return format!("{bt:?}");
    }

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
            // Stop after the first fixture frame — that's the entry
            // point into the scene closure; further frames are
            // #[test] wrappers that all point at the same file with
            // no extra signal.
            match classify(rel) {
                FrameKind::Other => continue,
                FrameKind::Fixture if seen_fixture_frame => break 'outer,
                FrameKind::Fixture => seen_fixture_frame = true,
                FrameKind::Src => {}
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
/// name, including occurrences inside generic parameters
/// (`frame<alloc::harness_tests::…>`). The test binary built from
/// `tests/alloc/main.rs` is named `alloc`, so the prefix is the same
/// everywhere and adds no information.
fn strip_test_crate_prefix(name: String) -> String {
    name.replace("alloc::", "")
}

/// Workspace-relative tail of a captured filename, or `None` if the path
/// isn't inside this crate. `backtrace`'s symbol resolver returns absolute
/// paths (`/home/.../palantir/src/widgets/button.rs`); strip the crate
/// root resolved at compile time so we don't depend on the project
/// directory's case or name (`Palantir` vs `palantir`, etc.).
fn user_relative(path: &str) -> Option<&str> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let stripped = path.strip_prefix(manifest)?;
    Some(stripped.strip_prefix('/').unwrap_or(stripped))
}
