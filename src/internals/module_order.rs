//! The module orders the `AGENTS.md` files state, held against every path of the shipped code: the
//! crate's top-level modules, and `gpu`'s submodules within it.

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
        "effect",
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

/// What sits outside the order: it does not ship in the default build, and each may import any
/// module it tests or measures. So may a bench file, so a whole-frame case sits beside the code it
/// measures. Refused: a `src/bench/` away from that code.
const OUTSIDE: [&str; 3] = ["golden", "internals", "bench"];

/// `gpu`'s submodules in order, as `src/gpu/AGENTS.md` draws it: the leaves; the device; the
/// texel geometry and the app's paint callbacks; the resources the pipelines draw from; the atlas
/// pool; the pipelines; the tenants that draw through them; a frame's passes; the surfaces it
/// paints; and the backend over them all.
const GPU_ORDER: [&[&str]; 11] = [
    &[
        "debug_marker",
        "flag_names",
        "render_owner_id",
        "spectrum_tables",
        "atlas_memory",
    ],
    &["error"],
    &["device"],
    &["texel", "gpu_paint"],
    &["resource"],
    &["atlas"],
    &["pipeline"],
    &["raster", "backdrop"],
    &["frame"],
    &["surface"],
    &["wgpu_backend"],
];

/// What sits outside `gpu`'s order: the module root, its tests and benches, and the test and bench
/// devices.
const GPU_OUTSIDE: [&str; 5] = ["mod", "tests", "bench", "test_gpu", "bench_gpu"];

/// One order: the path its modules sit under, its tiers, and what sits outside it.
#[derive(Clone, Copy, Debug)]
struct Order {
    prefix: &'static str,
    tiers: &'static [&'static [&'static str]],
    outside: &'static [&'static str],
}

const CRATE: Order = Order {
    prefix: "crate::",
    tiers: &ORDER,
    outside: &OUTSIDE,
};

const GPU: Order = Order {
    prefix: "crate::gpu::",
    tiers: &GPU_ORDER,
    outside: &GPU_OUTSIDE,
};

impl Order {
    fn tier(self, module: &str) -> Option<usize> {
        self.tiers.iter().position(|tier| tier.contains(&module))
    }
}

/// The lines of a file that ship: those before its gated tail, an inline `mod internals` or
/// `mod tests`, which may reach past the order as a bench and a test do. Only the inline forms
/// close a file: a gated `mod tests;` may sit above code that ships. By lines, so a checkout with
/// CRLF line endings, as a Windows runner's is, reads as any other.
fn shipped(text: &str) -> Vec<&str> {
    const TAILS: [[&str; 2]; 3] = [
        ["#[cfg(test)]", "mod tests {"],
        ["#[cfg(test)]", "pub(crate) mod internals {"],
        [
            "#[cfg(feature = \"internals\")]",
            "pub(crate) mod internals {",
        ],
    ];
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.windows(2).position(|pair| {
        TAILS
            .iter()
            .any(|[gate, item]| pair[0] == *gate && pair[1].starts_with(item))
    });
    lines[..tail.unwrap_or(lines.len())].to_vec()
}

/// Each module of `order` that `module`'s code `text` names and may not, and `{` for a grouped
/// path, which hides the modules it names.
fn edges_against_order<'a>(order: Order, module: &str, text: &'a str) -> Vec<&'a str> {
    let from = order.tier(module).expect("a module of the order");
    shipped(text)
        .into_iter()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(|line| {
            line.match_indices(order.prefix)
                .map(move |(at, _)| &line[at + order.prefix.len()..])
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
            *target == "{" || (*target != module && order.tier(target).is_some_and(|to| to >= from))
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
    assert_eq!(
        edges_against_order(CRATE, "ui", text),
        ["widget_core", "driver"]
    );
    assert_eq!(
        edges_against_order(CRATE, "damage", "use crate::input::f::F;\n"),
        ["input"]
    );
    assert_eq!(
        edges_against_order(CRATE, "ui", "use crate::{host::g::G, common::h::H};\n"),
        ["{"]
    );
    assert_eq!(
        edges_against_order(
            CRATE,
            "ui",
            "#[cfg(test)]\nmod tests;\nuse crate::host::i::I;\n"
        ),
        ["host"],
        "a gated declaration closes nothing"
    );
    assert_eq!(
        edges_against_order(
            CRATE,
            "ui",
            "use crate::widget_core::j::J;\r\n\
             #[cfg(test)]\r\nmod tests { use crate::host::k::K; }\r\n"
        ),
        ["widget_core"],
        "CRLF line endings close a file too"
    );
    assert_eq!(
        edges_against_order(
            CRATE,
            "ui",
            "#[cfg(feature = \"internals\")]\r\npub(crate) mod internals {\r\n\
             use crate::host::l::L;\r\n}\r\n"
        ),
        Vec::<&str>::new(),
        "and so does each other tail"
    );
}

/// Each module has a place, in its order or outside it, and no shipped line of one names a module it
/// may not: the crate's top-level modules over `src`, and `gpu`'s submodules over `src/gpu`.
#[test]
fn no_module_imports_against_the_order() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (order, root, files_min) in [(CRATE, src.clone(), 500), (GPU, src.join("gpu"), 100)] {
        let mut files = 0;
        for path in rust_files(&root) {
            let relative = path.strip_prefix(&root).unwrap();
            let top = relative.components().next().unwrap().as_os_str();
            let module = top.to_str().unwrap().trim_end_matches(".rs");
            if module == "lib" || order.outside.contains(&module) {
                continue;
            }
            assert!(
                order.tier(module).is_some(),
                "{module} has no place in the order"
            );
            if !ships(relative) {
                continue;
            }
            files += 1;
            let text = fs::read_to_string(&path).unwrap();
            let against = edges_against_order(order, module, &text);
            assert!(
                against.is_empty(),
                "{} imports {against:?} against the order",
                relative.display()
            );
        }
        assert!(
            files > files_min,
            "the scan reads the tree, not {files} files"
        );
    }
}

/// The gpu scan reads paths under `crate::gpu::` alone: an edge to a later submodule, and none to
/// an earlier one or past the module.
#[test]
fn the_gpu_scan_finds_an_edge_against_its_order() {
    let text = "use crate::gpu::resource::a::A;\n\
                use crate::gpu::wgpu_backend::B;\n\
                use crate::renderer::c::C;\n";
    assert_eq!(edges_against_order(GPU, "atlas", text), ["wgpu_backend"]);
}
