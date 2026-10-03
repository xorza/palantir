use super::*;
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
    assert_eq!(
        result,
        format!("{PRELUDE}const A: u32 = 7u; const B: f32 = 0.5;"),
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
    for &body in ShaderBody::VARIANTS {
        let (file, source) = (body, body.wgsl());
        let code = strip_comments(source);
        // Every marker is on a `const` line the name scan reads, so a
        // scan that stopped matching cannot pass by checking nothing.
        assert_eq!(
            source.lines().filter_map(pinned_const_name).count(),
            source.matches("/*{").count(),
            "{file:?}: a marker sits where the const-name scan does not see it",
        );
        for name in source.lines().filter_map(pinned_const_name) {
            let uses = code
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .filter(|word| *word == name)
                .count();
            assert!(
                uses > 1,
                "{file:?}: `{name}` is substituted from Rust and never read. Either the \
                 shader hard-codes the value it was given, or nothing should pin it.",
            );
        }
    }
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
