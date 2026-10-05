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

/// Every constant the Rust side substitutes is compared against
/// somewhere in the shader that declares it.
///
/// [`ShaderBody::specialize`]'s own assert proves a marker was *replaced*. It
/// cannot prove the value is *read*, and a constant that is declared
/// and never read is a pin the shader ignores: Rust believes it owns
/// the mapping while the shader has hard-coded a literal, and the two
/// drift the first time either side renumbers. Both halves of that
/// have happened here — `apply_spread` switched on `case 1u` beside
/// three substituted spread modes, and the curve fragment decoded its
/// join look as `kind - KIND_JOIN_ROUND` beside a substituted
/// `KIND_JOIN_BEVEL` it never mentioned.
///
/// A value the shader legitimately does not compare against — the
/// fall-through arm of a dispatch — must not be pinned at all. The
/// fix for a failure here is one or the other, never an exemption.
#[test]
fn every_pinned_shader_constant_is_read() {
    // Each body's own constants fill every marker it and the prelude
    // declare, exactly once — `specialize` panics otherwise — so a
    // constant added on one side only fails here, with no device.
    for &body in ShaderBody::VARIANTS {
        body.specialize();
    }
    let sources = ShaderBody::VARIANTS
        .iter()
        .map(|body| (format!("{body:?}"), body.wgsl()))
        .chain([("prelude".to_owned(), PRELUDE)]);
    for (file, source) in sources {
        let code = strip_comments(source);
        // Every marker is on a `const` line the name scan reads, so a
        // scan that stopped matching cannot pass by checking nothing.
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

/// The quad shader's `fs` never reaches the blurred-corner integral, and
/// `fs_shadow` does.
///
/// A pipeline gets the registers and code of everything its entry
/// reaches. With the integral reachable from `fs`, the pipeline of every
/// quad — a plain rectangle too — carried 56 VGPRs and 50 KB of code on
/// RDNA2 instead of 40 and 4 KB. The schedule routes shadows to
/// `fs_shadow`, so nothing else needs the integral.
#[test]
fn only_the_shadow_entry_reaches_the_blur_integral() {
    let source = strip_comments(&ShaderBody::Quad.specialize());
    let reaches = |entry: &str| reachable_functions(&source, entry);
    for heavy in ["blurred_box_coverage", "blurred_corner", "arc_half"] {
        assert!(!reaches("fs").contains(heavy), "`fs` reaches `{heavy}`");
        assert!(
            reaches("fs_shadow").contains(heavy),
            "`fs_shadow` lost `{heavy}`"
        );
    }
    assert!(
        reaches("fs").contains("sdf_rounded_rect"),
        "the call scan finds nothing"
    );
}

/// Every function `entry` calls, directly or through others, in a WGSL
/// `source` without comments. A call is a defined function's name followed
/// by `(`.
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

/// The name a `const NAME: T = /*{MARKER}*/;` line declares, or
/// `None` for any other line.
fn pinned_const_name(line: &str) -> Option<&str> {
    if !line.contains("/*{") {
        return None;
    }
    let declared = line.trim_start().strip_prefix("const ")?;
    Some(declared.split(':').next()?.trim())
}

/// `source` with both comment forms removed, so a constant named in
/// prose is not counted as a use — nor is the `/*{MARKER}*/` sitting
/// on the declaration line, which repeats the name it fills.
///
/// Line comments go first: no block comment in these sources
/// contains a `//`, while several lines carry both.
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
