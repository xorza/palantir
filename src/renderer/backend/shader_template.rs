//! Assembly of a pipeline's WGSL source: the shared prelude, then the
//! shader body with its Rust-owned constants substituted in.

/// Concatenated ahead of every shader body, so the vocabulary the five
/// pipelines share has one definition. See the file itself for what may
/// go in it.
const PRELUDE: &str = include_str!("prelude.wgsl");

/// The five shader bodies. Each pipeline names its own here rather than
/// writing an `include_str!` of its own, so
/// `every_pinned_shader_constant_is_read` covers exactly the sources the
/// backend compiles — a new pipeline that skips this module is a shader
/// nothing checks.
pub(super) const QUAD_WGSL: &str = include_str!("quad.wgsl");
pub(super) const MESH_WGSL: &str = include_str!("mesh.wgsl");
pub(super) const CURVE_WGSL: &str = include_str!("curve_pipeline/curve.wgsl");
pub(super) const IMAGE_WGSL: &str = include_str!("image_pipeline/image.wgsl");
pub(super) const RASTER_ATLAS_WGSL: &str = include_str!("raster_atlas/shader.wgsl");

#[derive(Debug)]
pub(super) struct ShaderConstant {
    marker: &'static str,
    value: String,
}

impl ShaderConstant {
    pub(super) fn uint(marker: &'static str, value: u32) -> Self {
        Self {
            marker,
            value: format!("{value}u"),
        }
    }

    pub(super) fn float(marker: &'static str, value: f32) -> Self {
        assert!(value.is_finite());
        Self {
            marker,
            value: format!("{value:?}"),
        }
    }
}

/// The complete source for one pipeline, ready to hand to
/// `create_shader_module`.
///
/// **Every shader in this backend is built here**, including the ones
/// with no constants to substitute — that is what puts [`PRELUDE`] in
/// front of all of them, and what makes an unsubstituted marker a
/// startup panic rather than a shader that compiles with a comment
/// where a number belongs.
pub(super) fn specialize(body: &str, constants: &[ShaderConstant]) -> String {
    let mut specialized = format!("{PRELUDE}{body}");
    for constant in constants {
        let marker = format!("/*{{{}}}*/", constant.marker);
        assert_eq!(
            specialized.matches(&marker).count(),
            1,
            "WGSL marker {marker} must occur exactly once",
        );
        specialized = specialized.replace(&marker, &constant.value);
    }
    assert!(
        !specialized.contains("/*{"),
        "WGSL template contains an unsubstituted constant marker",
    );
    specialized
}

#[cfg(test)]
mod tests {
    use super::{PRELUDE, ShaderConstant, specialize};

    #[test]
    fn specialization_replaces_every_typed_marker() {
        let result = specialize(
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
        specialize("const A: u32 = 1u;", &[ShaderConstant::uint("A", 7)]);
    }

    /// The sources the backend compiles, named for the failure message.
    const SHADERS: [(&str, &str); 5] = [
        ("quad.wgsl", super::QUAD_WGSL),
        ("mesh.wgsl", super::MESH_WGSL),
        ("curve.wgsl", super::CURVE_WGSL),
        ("image.wgsl", super::IMAGE_WGSL),
        ("raster_atlas.wgsl", super::RASTER_ATLAS_WGSL),
    ];

    /// Every constant the Rust side substitutes is compared against
    /// somewhere in the shader that declares it.
    ///
    /// [`specialize`]'s own assert proves a marker was *replaced*. It
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
        for (file, source) in SHADERS {
            let code = strip_comments(source);
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
}
