//! The module order `AGENTS.md` states, held against every `crate::` path of the shipped code.

use std::fs;
use std::path::{Path, PathBuf};

/// The order, each tier importing only the tiers before it, as `AGENTS.md` draws it.
const ORDER: [&[&str]; 17] = [
    &["common"],
    &["primitives"],
    &[
        "animation",
        "text",
        "display",
        "window",
        "clipboard",
        "image_registry",
        "diagnostics",
    ],
    &["shape"],
    &["icons"],
    &["scene"],
    &["layout"],
    &["cascade"],
    &["damage", "input"],
    &["renderer"],
    &["gpu"],
    &["theme"],
    &["ui"],
    &["widget_core"],
    &["widgets"],
    &["driver"],
    &["host"],
];

/// What `AGENTS.md` sets outside the order: it does not ship, and may import any module.
const OUTSIDE: [&str; 3] = ["golden", "internals", "bench"];

fn tier(module: &str) -> Option<usize> {
    ORDER.iter().position(|tier| tier.contains(&module))
}

/// The code of a file that ships: the text before its gated tail, an inline `mod internals` or
/// `mod tests`, which may reach past the order as a bench and a test do. Only the inline forms
/// close a file: a gated `mod tests;` may sit above code that ships.
fn shipped(text: &str) -> &str {
    let tail = [
        "#[cfg(test)]\nmod tests {",
        "#[cfg(test)]\npub(crate) mod internals {",
        "#[cfg(feature = \"internals\")]\npub(crate) mod internals {",
    ]
    .iter()
    .filter_map(|gate| text.find(gate))
    .min();
    &text[..tail.unwrap_or(text.len())]
}

/// Each module of the order that `module`'s code `text` names and may not, and `{` for a grouped
/// path, which hides the modules it names.
fn edges_against_order<'a>(module: &str, text: &'a str) -> Vec<&'a str> {
    let from = tier(module).expect("a module of the order");
    shipped(text)
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(|line| {
            line.match_indices("crate::")
                .map(move |(at, _)| &line[at + 7..])
        })
        .map(|path| {
            if path.starts_with('{') {
                return "{";
            }
            path.split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("")
        })
        .filter(|target| {
            *target == "{" || (*target != module && tier(target).is_some_and(|to| to >= from))
        })
        .collect()
}

/// A bench, a test or a sweep sits outside the order wherever its file lies.
fn ships(path: &Path) -> bool {
    !path.components().any(|part| {
        let part = part.as_os_str();
        ["tests", "tests.rs", "bench.rs", "sweep"]
            .iter()
            .any(|name| part == *name)
    })
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files
}

/// The scan names what it should: an edge to the right of a module and an edge inside a tier, and
/// not an edge to the left, one of the module itself, a crate-root item, a comment or a gated
/// tail.
#[test]
fn the_scan_finds_an_edge_against_the_order() {
    let text = "use crate::widget_core::a::A;\n\
                use crate::theme::Theme;\n\
                use crate::ui::b::B;\n\
                use crate::App;\n\
                // crate::host::c\n\
                fn f() { crate::driver::d(); }\n\
                #[cfg(test)]\nmod tests { use crate::widgets::e::E; }\n";
    assert_eq!(edges_against_order("ui", text), ["widget_core", "driver"]);
    assert_eq!(
        edges_against_order("damage", "use crate::input::f::F;\n"),
        ["input"]
    );
    assert_eq!(
        edges_against_order("ui", "use crate::{host::g::G, common::h::H};\n"),
        ["{"]
    );
    assert_eq!(
        edges_against_order("ui", "#[cfg(test)]\nmod tests;\nuse crate::host::i::I;\n"),
        ["host"],
        "a gated declaration closes nothing"
    );
}

/// Each top-level module has a place, in the order or outside it, and no shipped line of one names
/// a module it may not.
#[test]
fn no_module_imports_against_the_order() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = 0;
    for path in rust_files(&root) {
        let relative = path.strip_prefix(&root).unwrap();
        let top = relative.components().next().unwrap().as_os_str();
        let module = top.to_str().unwrap().trim_end_matches(".rs");
        if module == "lib" || OUTSIDE.contains(&module) {
            continue;
        }
        assert!(tier(module).is_some(), "{module} has no place in the order");
        if !ships(relative) {
            continue;
        }
        files += 1;
        let text = fs::read_to_string(&path).unwrap();
        let against = edges_against_order(module, &text);
        assert!(
            against.is_empty(),
            "{} imports {against:?} against the order",
            relative.display()
        );
    }
    assert!(files > 500, "the scan reads the crate, not {files} files");
}
