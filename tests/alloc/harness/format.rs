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
use std::path::{self, Path};

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
            match classify(&rel) {
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

/// Crate-relative tail of a captured filename, or `None` if the path isn't
/// inside this crate, with separators normalised to `/`.
///
/// The platforms hand back different shapes for the crate's *own* files.
/// DWARF joins each name onto the compilation directory, so Linux and macOS
/// report an absolute path (`/home/.../palantir/src/widgets/button.rs`);
/// strip the crate root resolved at compile time, so we don't depend on the
/// project directory's case or name (`Palantir` vs `palantir`). A PDB keeps
/// what cargo passed instead, so Windows reports the same file already
/// relative and backslash-separated, sometimes led by a `.` component.
///
/// An absolute path with no manifest prefix belongs to a dependency or to
/// std, so reject it. A relative one can only be ours: cargo passes every
/// dependency by absolute path.
fn user_relative(path: &str) -> Option<String> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let rel = match path.strip_prefix(manifest) {
        Some(tail) => tail.trim_start_matches(path::is_separator),
        None if Path::new(path).is_absolute() => return None,
        None => path,
    };
    // `MAIN_SEPARATOR` is already `/` off Windows, so this is a no-op there.
    let rel = rel.replace(path::MAIN_SEPARATOR, "/");
    Some(rel.trim_start_matches("./").to_owned())
}

mod tests {
    use super::{FrameKind, classify, user_relative};
    use std::path::{MAIN_SEPARATOR_STR, Path};

    /// Rewrite `/` as whatever the running platform's debug info would carry,
    /// so one table covers the DWARF and the PDB shape at once.
    fn native(path: &str) -> String {
        path.replace('/', MAIN_SEPARATOR_STR)
    }

    /// Whether the frame filter would print this file at all.
    fn kept(path: &str) -> bool {
        user_relative(path).is_some_and(|rel| !matches!(classify(&rel), FrameKind::Other))
    }

    #[test]
    fn user_relative_normalises_every_shape_of_our_own_files() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        // DWARF: the file name joined onto the compilation directory.
        let absolute = native(&format!("{manifest}/src/widgets/button.rs"));
        // PDB: what cargo passed, sometimes led by a `.` component.
        for path in [absolute, native("./src/widgets/button.rs")] {
            assert_eq!(
                user_relative(&path).as_deref(),
                Some("src/widgets/button.rs"),
                "unexpected tail for {path}",
            );
        }
        assert_eq!(
            user_relative(&native("tests/alloc/fixtures/text.rs")).as_deref(),
            Some("tests/alloc/fixtures/text.rs"),
        );
    }

    #[test]
    fn user_relative_keeps_crate_files_and_drops_everyone_else() {
        assert!(kept(&native("src/widgets/button.rs")));
        assert!(kept(&native("tests/alloc/fixtures/text.rs")));
        // Harness machinery is ours, and still not worth a frame.
        assert!(!kept(&native("tests/alloc/harness/format.rs")));
        // A sibling crate built by path: absolute, and not under the manifest.
        let outside = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the manifest directory has a parent")
            .join("not-palantir")
            .join("src/lib.rs");
        assert!(!kept(&outside.to_string_lossy()));
        // std ships a path that is absolute on unix and rootless on Windows,
        // so this one is rejected by the manifest prefix on one platform and
        // by `src/` not matching `/rustc/...` on the other.
        assert!(!kept(
            "/rustc/0000000000000000000000000000000000000000/library/std/src/rt.rs"
        ));
    }
}
