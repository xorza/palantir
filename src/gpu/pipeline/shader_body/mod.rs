//! Assembly of a pipeline's WGSL source: the shared prelude, then the
//! shader body, with the Rust-owned constants of both substituted in.

use crate::gpu::raster::raster_atlas::raster_quad::{
    FLAG_COLOR, FLAG_DESATURATE, FLAG_MASK, U_BITS, V_SHIFT,
};
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::primitives::paint::brush::gradient::Spread;
use crate::renderer::render_buffer::curve::SEGMENTS_PER_INSTANCE;
use crate::renderer::render_buffer::curve_caps::CurveCaps;
use crate::renderer::render_buffer::curve_kind::CurveKind;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::stroke_bounds::MITER_LIMIT;
use crate::shape::style::LineCap;

/// Concatenated ahead of every shader body, so the vocabulary the
/// pipelines share has one definition. See the file itself for what may
/// go in it.
const PRELUDE: &str = include_str!("../prelude.wgsl");

/// The shader bodies the backend compiles, each with its label and the
/// Rust-owned constants it takes. A pipeline builds its module through
/// [`Self::module`] rather than writing an `include_str!` of its own, and
/// the tests specialize every variant — so a new shader joins the checks
/// by existing, and the matches below will not compile until they name
/// its file, its label and its constants.
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

    const fn label(self) -> &'static str {
        match self {
            Self::Quad => "palantir.quad.shader",
            Self::Mesh => "palantir.mesh.shader",
            Self::Curve => "palantir.curve.shader",
            Self::Image => "palantir.image.shader",
            Self::RasterAtlas => "palantir.raster.shader",
            Self::Blit => "palantir.blit.shader",
        }
    }

    /// The numbers this body declares as markers, from the Rust types that
    /// own them.
    fn constants(self) -> Vec<ShaderConstant> {
        match self {
            Self::Quad => vec![
                ShaderConstant::uint("FILL_TAG_MASK", FillKind::TAG_MASK),
                ShaderConstant::uint("SPREAD_SHIFT", FillKind::SPREAD_SHIFT),
                ShaderConstant::uint("SPREAD_MASK", FillKind::SPREAD_MASK),
                ShaderConstant::uint("BRUSH_KIND_SOLID", FillKind::TAG_SOLID),
                ShaderConstant::uint("BRUSH_KIND_LINEAR", FillKind::TAG_LINEAR),
                ShaderConstant::uint("BRUSH_KIND_RADIAL", FillKind::TAG_RADIAL),
                ShaderConstant::uint("BRUSH_KIND_CONIC", FillKind::TAG_CONIC),
                ShaderConstant::uint("BRUSH_KIND_SHADOW_DROP", FillKind::TAG_SHADOW_DROP),
                ShaderConstant::uint("BRUSH_KIND_SHADOW_INSET", FillKind::TAG_SHADOW_INSET),
                ShaderConstant::uint("BRUSH_KIND_TRIANGLE", FillKind::TAG_TRIANGLE),
                ShaderConstant::uint("FILL_FLAG_FAST", FillKind::FAST_BIT),
                ShaderConstant::uint("FILL_FLAG_WINDOW", FillKind::WINDOW_BIT),
                // `Pad` is not pinned: it is `apply_spread`'s fallback,
                // which is also the right answer for a mode the shader
                // does not know, so nothing there compares against it.
                ShaderConstant::uint("SPREAD_REPEAT", Spread::Repeat as u32),
                ShaderConstant::uint("SPREAD_REFLECT", Spread::Reflect as u32),
            ],
            Self::Curve => vec![
                ShaderConstant::uint("SEGMENTS_PER_INSTANCE", SEGMENTS_PER_INSTANCE),
                ShaderConstant::float("MITER_LIMIT", MITER_LIMIT),
                ShaderConstant::uint("CAP_MASK", CurveCaps::CAP_MASK),
                ShaderConstant::uint("CAP_AT_START", CurveCaps::AT_START),
                ShaderConstant::uint("CAP_AT_END", CurveCaps::AT_END),
                ShaderConstant::uint("CAP_BUTT", LineCap::Butt as u32),
                ShaderConstant::uint("CAP_ROUND", LineCap::Round as u32),
                ShaderConstant::uint("KIND_ARC", CurveKind::ARC.bits()),
                ShaderConstant::uint("KIND_SEGMENT", CurveKind::SEGMENT.bits()),
                ShaderConstant::uint("KIND_JOIN_ROUND", CurveKind::JOIN_ROUND.bits()),
                ShaderConstant::uint("KIND_JOIN_BEVEL", CurveKind::JOIN_BEVEL.bits()),
                ShaderConstant::uint("KIND_JOIN_MITER", CurveKind::JOIN_MITER.bits()),
                ShaderConstant::uint("FILL_TAG_MASK", FillKind::TAG_MASK),
                ShaderConstant::uint("BRUSH_KIND_RAMP", FillKind::TAG_RAMP),
            ],
            Self::Image => vec![
                ShaderConstant::uint("IMG_FLAG_TILED", ImageFlags::TILED.bits()),
                ShaderConstant::uint("IMG_FLAG_MIN_NEAREST", ImageFlags::MIN_NEAREST.bits()),
                ShaderConstant::uint("IMG_FLAG_MAG_NEAREST", ImageFlags::MAG_NEAREST.bits()),
                ShaderConstant::uint("IMG_FLAG_TAPS_MEAN", ImageFlags::TAPS_MEAN.bits()),
                ShaderConstant::uint("IMG_FLAG_TAPS_PEAK", ImageFlags::TAPS_PEAK.bits()),
            ],
            Self::RasterAtlas => vec![
                ShaderConstant::uint("U_BITS", U_BITS),
                ShaderConstant::uint("V_SHIFT", V_SHIFT),
                ShaderConstant::uint("FLAG_MASK", FLAG_MASK),
                ShaderConstant::uint("FLAG_DESATURATE", FLAG_DESATURATE),
                ShaderConstant::uint("FLAG_COLOR", FLAG_COLOR),
            ],
            Self::Mesh | Self::Blit => Vec::new(),
        }
    }

    /// The complete source for this body: [`PRELUDE`] in front, every
    /// constant substituted.
    ///
    /// **Every shader in this backend is built here**, including the
    /// ones with no constants of their own — that is what puts
    /// [`PRELUDE`] in front of all of them, and what makes an
    /// unsubstituted marker a panic rather than a shader that compiles
    /// with a comment where a number belongs.
    fn specialize(self) -> String {
        specialize_source(self.wgsl(), &self.constants())
    }

    /// The shader module for this body.
    pub(crate) fn module(self, device: &wgpu::Device) -> wgpu::ShaderModule {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(self.label()),
            source: wgpu::ShaderSource::Wgsl(self.specialize().into()),
        })
    }
}

#[derive(Debug)]
struct ShaderConstant {
    marker: &'static str,
    value: String,
}

impl ShaderConstant {
    fn uint(marker: &'static str, value: u32) -> Self {
        Self {
            marker,
            value: format!("{value}u"),
        }
    }

    fn float(marker: &'static str, value: f32) -> Self {
        assert!(value.is_finite());
        Self {
            marker,
            value: format!("{value:?}"),
        }
    }
}

/// The constants [`PRELUDE`] declares, substituted into every shader.
fn prelude_constants() -> [ShaderConstant; 1] {
    [ShaderConstant::float("AA_HALF_WIDTH", AA_HALF_WIDTH)]
}

fn specialize_source(body: &str, constants: &[ShaderConstant]) -> String {
    let mut specialized = format!("{PRELUDE}{body}");
    for constant in prelude_constants().iter().chain(constants) {
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
