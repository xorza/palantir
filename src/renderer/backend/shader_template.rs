//! Checked substitution for Rust-owned constants embedded in WGSL sources.

#[derive(Debug)]
pub(crate) struct ShaderConstant {
    marker: &'static str,
    value: String,
}

impl ShaderConstant {
    pub(crate) fn uint(marker: &'static str, value: u32) -> Self {
        Self {
            marker,
            value: format!("{value}u"),
        }
    }

    pub(crate) fn float(marker: &'static str, value: f32) -> Self {
        assert!(value.is_finite());
        Self {
            marker,
            value: format!("{value:?}"),
        }
    }
}

pub(crate) fn specialize(source: &str, constants: &[ShaderConstant]) -> String {
    let mut specialized = source.to_owned();
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
    use super::{ShaderConstant, specialize};

    #[test]
    fn specialization_replaces_every_typed_marker() {
        let source = "const A: u32 = /*{A}*/; const B: f32 = /*{B}*/;";
        let result = specialize(
            source,
            &[
                ShaderConstant::uint("A", 7),
                ShaderConstant::float("B", 0.5),
            ],
        );
        assert_eq!(result, "const A: u32 = 7u; const B: f32 = 0.5;");
    }

    #[test]
    #[should_panic(expected = "must occur exactly once")]
    fn specialization_rejects_missing_marker() {
        specialize("const A: u32 = 1u;", &[ShaderConstant::uint("A", 7)]);
    }
}
