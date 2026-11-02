//! The guard that keeps wgpu inside `crate::gpu`: `clippy.toml` lists every wgpu type the crate
//! names, as clippy matches each entry by exact path.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// The `wgpu` paths that name a type or a trait in `text`: each path up to its first segment that
/// starts in capitals and is not a constant, through `use wgpu::{…}` groups at any depth.
fn named_types(text: &str, found: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut at = 0;
    while let Some(offset) = text[at..].find("wgpu::") {
        let start = at + offset;
        at = start + "wgpu::".len();
        let joined = start > 0 && {
            let before = bytes[start - 1];
            before.is_ascii_alphanumeric() || before == b'_' || before == b':'
        };
        if !joined {
            walk("wgpu", &text[at..], found);
        }
    }
}

/// Follow the path after `prefix::` in `rest`.
fn walk(prefix: &str, rest: &str, found: &mut BTreeSet<String>) {
    if let Some(group) = rest.strip_prefix('{') {
        let mut depth = 1;
        let end = group
            .char_indices()
            .find(|&(_, c)| {
                depth += match c {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                };
                depth == 0
            })
            .map_or(group.len(), |(i, _)| i);
        let mut depth = 0;
        let mut item = String::new();
        for c in group[..end].chars().chain([',']) {
            match c {
                ',' if depth == 0 => {
                    let path = item.split(" as ").next().unwrap_or_default().trim();
                    if !path.is_empty() && path != "self" {
                        walk(prefix, path, found);
                    }
                    item.clear();
                }
                _ => {
                    depth += match c {
                        '{' => 1,
                        '}' => -1,
                        _ => 0,
                    };
                    item.push(c);
                }
            }
        }
        return;
    }
    let len = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let segment = &rest[..len];
    if segment.is_empty() {
        return;
    }
    let path = format!("{prefix}::{segment}");
    let constant = segment.chars().all(|c| !c.is_ascii_lowercase());
    if segment.starts_with(|c: char| c.is_ascii_uppercase()) {
        if !constant {
            found.insert(path);
        }
        return;
    }
    if let Some(next) = rest[len..].strip_prefix("::") {
        walk(&path, next, found);
    }
}

fn source_files(dir: &Path, files: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("the source tree reads") {
        let path = entry.expect("a source entry reads").path();
        if path.is_dir() {
            source_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(fs::read_to_string(&path).expect("a source file reads"));
        }
    }
}

/// Every wgpu type and trait a file under `src/` names is in `clippy.toml`'s `disallowed-types`,
/// and every wgpu entry there names one: a type the list misses is free to leave `crate::gpu`, and
/// an entry nothing names is a stale one.
#[test]
fn clippy_guards_every_wgpu_type_the_crate_names() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    source_files(&root.join("src"), &mut files);
    let mut named = BTreeSet::new();
    for file in &files {
        named_types(file, &mut named);
    }
    let config = fs::read_to_string(root.join("clippy.toml")).expect("clippy.toml reads");
    let types = config
        .split("disallowed-methods")
        .next()
        .expect("a split yields its first part");
    let listed: BTreeSet<String> = types
        .split("path = \"")
        .skip(1)
        .filter_map(|entry| entry.split('"').next())
        .filter(|path| path.starts_with("wgpu::"))
        .map(str::to_owned)
        .collect();
    let unguarded: Vec<_> = named.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&named).collect();
    assert!(unguarded.is_empty(), "add to clippy.toml: {unguarded:?}");
    assert!(stale.is_empty(), "no file names: {stale:?}");
}

/// The scan reads a path up to its type, a group at any depth, a renamed item, and skips a
/// constant and a longer name that ends in `wgpu`.
#[test]
fn the_scan_reads_paths_and_groups() {
    let mut found = BTreeSet::new();
    named_types(
        "use wgpu::{util::{DeviceExt, StagingBelt}, Device as D, self}; \
         wgpu::TextureFormat::Rgba8Unorm; wgpu::COPY_BUFFER_ALIGNMENT; mywgpu::Fake;",
        &mut found,
    );
    let want = [
        "wgpu::Device",
        "wgpu::TextureFormat",
        "wgpu::util::DeviceExt",
        "wgpu::util::StagingBelt",
    ];
    assert_eq!(found, want.map(str::to_owned).into());
}
