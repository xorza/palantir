//! Assembly of a pipeline's WGSL source: the shared prelude, then the
//! shader body with its Rust-owned constants substituted in.

/// Concatenated ahead of every shader body, so the vocabulary the
/// pipelines share has one definition. See the file itself for what may
/// go in it.
const PRELUDE: &str = include_str!("../prelude.wgsl");

/// The shader bodies the backend compiles. A pipeline names its own here
/// rather than writing an `include_str!` of its own, and the
/// `every_pinned_shader_constant_is_read` test walks every variant — so a
/// new shader joins the check by existing, and the match below will not
/// compile until it names its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::VariantArray)]
pub(crate) enum ShaderBody {
    Quad,
    Mesh,
    Curve,
    Image,
    RasterAtlas,
    Blit,
}

impl ShaderBody {
    const fn wgsl(self) -> &'static str {
        match self {
            Self::Quad => include_str!("../quad_pipeline/shader.wgsl"),
            Self::Mesh => include_str!("../mesh_pipeline/shader.wgsl"),
            Self::Curve => include_str!("../curve_pipeline/shader.wgsl"),
            Self::Image => include_str!("../image_pipeline/shader.wgsl"),
            Self::RasterAtlas => include_str!("../../raster/raster_atlas/shader.wgsl"),
            Self::Blit => include_str!("../blit_pipeline/shader.wgsl"),
        }
    }

    /// The complete source for this pipeline, ready to hand to
    /// `create_shader_module`.
    ///
    /// **Every shader in this backend is built here**, including the
    /// ones with no constants to substitute — that is what puts
    /// [`PRELUDE`] in front of all of them, and what makes an
    /// unsubstituted marker a startup panic rather than a shader that
    /// compiles with a comment where a number belongs.
    pub(crate) fn specialize(self, constants: &[ShaderConstant]) -> String {
        specialize_source(self.wgsl(), constants)
    }
}

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

    pub(super) fn float(marker: &'static str, value: f32) -> Self {
        assert!(value.is_finite());
        Self {
            marker,
            value: format!("{value:?}"),
        }
    }
}

fn specialize_source(body: &str, constants: &[ShaderConstant]) -> String {
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
mod tests;
