//! Backtrace filter and pretty-printer for audit failures: resolves backtraces lazily and keeps only palantir `src/...` frames and the fixture entry point; names lose the `alloc::` prefix and `::h<hash>` suffix.

use backtrace::Backtrace;
use std::env;
use std::fmt::Write as _;
use std::path::{self, Path};

/// Render `bt` as a call stack from fixture closure down to the allocating call site; `PALANTIR_ALLOC_FULL_BT=1` dumps the raw backtrace instead.
pub(crate) fn user_frames(bt: &mut Backtrace) -> String {
    bt.resolve();
    if env::var_os("PALANTIR_ALLOC_FULL_BT").is_some() {
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
            // Stop after the first fixture frame; further frames are `#[test]` wrappers with no extra signal.
            match classify(&rel) {
                FrameKind::Other => continue,
                FrameKind::Fixture if seen_fixture_frame => break 'outer,
                FrameKind::Fixture => seen_fixture_frame = true,
                FrameKind::Src => {}
            }
            let name = symbol.name().map(|n| format!("{n:#}")).map_or_else(
                || String::from("<unknown>"),
                |n| strip_test_crate_prefix(&n),
            );
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
    /// Fixture entry point under `tests/alloc/fixtures/...`, kept once per trace.
    Fixture,
    /// Everything else (harness internals, non-crate code).
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

/// Drop the `alloc::` test-binary prefix from a demangled symbol, including inside generic parameters.
fn strip_test_crate_prefix(name: &str) -> String {
    name.replace("alloc::", "")
}

/// Crate-relative tail of a captured filename, or `None` outside this crate, with `/` separators. DWARF reports an absolute path (strip the compile-time crate root, so project directory case does not matter); a PDB reports a relative, backslash-separated one, maybe led by `.`. An absolute path with no manifest prefix is a dependency or std; a relative one is ours, since cargo passes dependencies by absolute path.
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

#[cfg(test)]
mod tests {
    use super::{FrameKind, classify, user_relative};
    use std::path::{MAIN_SEPARATOR_STR, Path};

    /// Rewrite `/` as the platform's separator so one table covers the DWARF and PDB shapes.
    fn native(path: &str) -> String {
        path.replace('/', MAIN_SEPARATOR_STR)
    }

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
        // std's path is absolute on unix and rootless on Windows, so it is rejected by manifest prefix on one and by `src/` not matching `/rustc/...` on the other.
        assert!(!kept(
            "/rustc/0000000000000000000000000000000000000000/library/std/src/rt.rs"
        ));
    }
}
