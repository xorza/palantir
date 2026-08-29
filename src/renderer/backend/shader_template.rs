//! Assembly of a pipeline's WGSL source: the shared prelude, then the
//! shader body with its Rust-owned constants substituted in.

/// Concatenated ahead of every shader body, so the vocabulary the five
/// pipelines share has one definition. See the file itself for what may
/// go in it.
const PRELUDE: &str = include_str!("prelude.wgsl");

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
}
