use crate::gpu::pipeline::internals::SHADERS;
use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::pipeline::shader_body::{PRELUDE, ShaderConstant, specialize_source};
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[test]
fn specialization_replaces_every_typed_marker() {
    let result = specialize_source(
        "const A: u32 = /*{A}*/; const B: f32 = /*{B}*/;",
        &[
            ShaderConstant::uint("A", 7),
            ShaderConstant::float("B", 0.5),
        ],
    );
    let prelude = PRELUDE.replace("/*{AA_HALF_WIDTH}*/", &format!("{AA_HALF_WIDTH:?}"));
    assert_eq!(
        result,
        format!("{prelude}const A: u32 = 7u; const B: f32 = 0.5;"),
    );
}

#[test]
#[should_panic(expected = "must occur exactly once")]
fn specialization_rejects_missing_marker() {
    specialize_source("const A: u32 = 1u;", &[ShaderConstant::uint("A", 7)]);
}

/// Every WGSL file of the backend is a body, an include or the prelude of [`SHADERS`], so no shader
/// skips the tests that walk the list.
#[test]
fn every_shader_file_is_walked() {
    let walked: HashSet<&str> = SHADERS
        .iter()
        .flat_map(|body| body.includes.iter().copied().chain([body.wgsl]))
        .chain([PRELUDE])
        .collect();
    let mut dirs = vec![PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/gpu"
    ))];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "wgsl") {
                let source = fs::read_to_string(&path).unwrap();
                assert!(
                    walked.contains(source.as_str()),
                    "{} is in no shader of `SHADERS`",
                    path.display(),
                );
            }
        }
    }
}

/// Every constant Rust substitutes must be read by its shader; an unread pin means the shader
/// hard-codes the value and the two drift on renumbering. Unpin rather than exempt.
#[test]
fn every_pinned_shader_constant_is_read() {
    for body in SHADERS {
        body.parse();
    }
    let sources = SHADERS
        .iter()
        .flat_map(|body| {
            body.includes
                .iter()
                .chain([&body.wgsl])
                .map(|&wgsl| (body.label, wgsl))
        })
        .chain([("prelude", PRELUDE)]);
    for (file, source) in sources {
        let code = strip_comments(source);
        assert_eq!(
            source.lines().filter_map(pinned_const_name).count(),
            source.matches("/*{").count(),
            "{file}: a marker sits where the const-name scan does not see it",
        );
        for name in source.lines().filter_map(pinned_const_name) {
            let uses = code
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .filter(|word| *word == name)
                .count();
            assert!(
                uses > 1,
                "{file}: `{name}` is substituted from Rust and never read. Either the \
                 shader hard-codes the value it was given, or nothing should pin it.",
            );
        }
    }
}

/// `fs` must not reach the blurred-corner integral (`fs_shadow` does, `fs_shadow_tables` only the
/// tabled form): reachable code costs registers (56 VGPRs and 50 KB vs 40 and 4 KB on RDNA2).
#[test]
fn only_the_shadow_entries_reach_the_blur_integral() {
    let module = QuadPipeline::SHADER.parse();
    let reaches = |entry: &str| reachable_functions(&module, entry);
    for heavy in [
        "blurred_box_coverage",
        "blurred_corner",
        "arc_half",
        "cutout_box_coverage",
        "corner_cutout",
        "shadow_coverage",
    ] {
        assert!(!reaches("fs").contains(heavy), "`fs` reaches `{heavy}`");
        assert!(
            !reaches("fs_shadow_tables").contains(heavy),
            "`fs_shadow_tables` reaches `{heavy}`"
        );
        assert!(
            reaches("fs_shadow").contains(heavy),
            "`fs_shadow` lost `{heavy}`"
        );
    }
    for tabled in [
        "tabled_coverage",
        "tabled_cutout",
        "cutout_lookup",
        "tables_edge_cdf",
        "filter_cdf_series",
        "filter_cdf_difference",
    ] {
        assert!(!reaches("fs").contains(tabled), "`fs` reaches `{tabled}`");
        assert!(
            reaches("fs_shadow_tables").contains(tabled),
            "`fs_shadow_tables` lost `{tabled}`"
        );
    }
    assert!(
        reaches("fs").contains("sdf_rounded_rect"),
        "the call scan finds nothing"
    );
}

/// The functions entry point `entry` of `module` calls, directly or through others, as naga's
/// call statements name them.
fn reachable_functions(module: &naga::Module, entry: &str) -> HashSet<String> {
    let entry = module
        .entry_points
        .iter()
        .find(|point| point.name == entry)
        .unwrap_or_else(|| panic!("no entry point `{entry}`"));
    let mut seen = HashSet::new();
    let mut stack = vec![&entry.function.body];
    while let Some(block) = stack.pop() {
        for statement in block {
            match statement {
                naga::Statement::Call { function, .. } => {
                    let called = &module.functions[*function];
                    let name = called.name.clone().expect("a named function");
                    if seen.insert(name) {
                        stack.push(&called.body);
                    }
                }
                naga::Statement::Block(inner) => stack.push(inner),
                naga::Statement::If { accept, reject, .. } => stack.extend([accept, reject]),
                naga::Statement::Switch { cases, .. } => {
                    stack.extend(cases.iter().map(|case| &case.body));
                }
                naga::Statement::Loop {
                    body, continuing, ..
                } => stack.extend([body, continuing]),
                _ => {}
            }
        }
    }
    seen
}

fn pinned_const_name(line: &str) -> Option<&str> {
    if !line.contains("/*{") {
        return None;
    }
    let declared = line.trim_start().strip_prefix("const ")?;
    Some(declared.split(':').next()?.trim())
}

/// `source` without comments, so prose and `/*{MARKER}*/` are not counted as uses.
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let mut code = line.split("//").next().unwrap_or("");
        while let Some(start) = code.find("/*") {
            let Some(len) = code[start..].find("*/") else {
                break;
            };
            out.push_str(&code[..start]);
            code = &code[start + len + 2..];
        }
        out.push_str(code);
        out.push('\n');
    }
    out
}
