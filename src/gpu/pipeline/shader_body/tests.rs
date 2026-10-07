use super::*;
use std::collections::{HashMap, HashSet};
use strum::VariantArray;

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

/// Every constant Rust substitutes must be read by its shader; an unread pin means the shader
/// hard-codes the value and the two drift on renumbering. Unpin rather than exempt.
#[test]
fn every_pinned_shader_constant_is_read() {
    for &body in ShaderBody::VARIANTS {
        body.specialize();
    }
    let sources = ShaderBody::VARIANTS
        .iter()
        .map(|body| (format!("{body:?}"), body.wgsl()))
        .chain([("prelude".to_owned(), PRELUDE)]);
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
    let source = strip_comments(&ShaderBody::Quad.specialize());
    let reaches = |entry: &str| reachable_functions(&source, entry);
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

fn reachable_functions(source: &str, entry: &str) -> HashSet<String> {
    let bodies: HashMap<&str, &str> = source
        .split("fn ")
        .skip(1)
        .filter_map(|def| {
            let name = def.split('(').next()?.trim();
            let open = def.find('{')?;
            let mut depth = 0;
            let close = def[open..].char_indices().find_map(|(i, c)| {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(open + i)
            })?;
            Some((name, &def[open..close]))
        })
        .collect();
    let mut seen = HashSet::new();
    let mut stack = vec![entry.to_owned()];
    while let Some(name) = stack.pop() {
        let body = bodies
            .get(name.as_str())
            .unwrap_or_else(|| panic!("no function `{name}`"));
        let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        let mut at = 0;
        while let Some(start) = body[at..].find(|c: char| c.is_ascii_alphabetic() || c == '_') {
            let start = at + start;
            let end = body[start..]
                .find(|c| !is_word(c))
                .map_or(body.len(), |len| start + len);
            let word = &body[start..end];
            if bodies.contains_key(word)
                && body[end..].trim_start().starts_with('(')
                && seen.insert(word.to_owned())
            {
                stack.push(word.to_owned());
            }
            at = end;
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
