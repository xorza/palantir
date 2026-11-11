//! [`ClosedEffect`]: an effect over a closed shape, an app's `EffectPaint` or a box's material,
//! which palantir's stage wraps.

use crate::common::span::Span;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::{
    ClosedPaintPayload, DrawClosedPayload,
};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::paint::capture_params::CaptureReach;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;

/// An effect over a closed shape: an app's paint, or a material, which carries its stage.
/// `params` indexes the window's [`RecordStore`](crate::shape::record_store::RecordStore)
/// parameter words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClosedEffect {
    pub(crate) effect: EffectId,
    pub(crate) params: Span,
    /// Its images, indexed into the record store's image column.
    pub(crate) images: Span,
    /// A backdrop effect's backdrop; `None` for a color effect.
    pub(crate) backdrop: Option<EffectCaptureDraw>,
    /// A material's stage around what the effect shows; `None` for an app's paint.
    pub(crate) stage: Option<MaterialStage>,
    /// A color effect's halo, logical px past the shape, outside it alone; `None` paints over
    /// the shape.
    pub(crate) outside: Option<f32>,
    /// A paint animation's fade, which scales the coverage, a material's stage alike.
    pub(crate) opacity: f32,
    /// Seconds into the paint animation's pass, which the effect reads; 0 when unanimated.
    pub(crate) time: f32,
}

/// What a backdrop effect's draw reads: what is painted under it, by the ticket the backdrop
/// ledger wrote for it, up to its reach past its rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectCaptureDraw {
    pub(crate) ticket: BackdropTicket,
    pub(crate) reach: CaptureReach,
}

/// What palantir lays around a material's effect: the luminance bounds it holds what the effect
/// shows to, then the box's fill, a color or a gradient, and its border over the whole shape. A
/// material box's fill and border live here, not in a `ClosedFill`, as the material draws them in
/// its own draw (`src/shape/AGENTS.md`, "A box's fill reaches under its border, a material under
/// both, in one draw"); the two routes share one composite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MaterialStage {
    pub(crate) fill: GpuFill,
    /// The fill's gradient geometry in the box's 0..1 frame; zero for a solid.
    pub(crate) fill_axis: FillAxis,
    /// Normalized by [`LoweredBorder::normalized`], so `NONE` means no border exactly.
    pub(crate) border: LoweredBorder,
    pub(crate) max_luminance: f32,
    pub(crate) min_luminance: f32,
}

impl ClosedEffect {
    /// A material reading `read`: its effect under its stage, the shape's `fill` and `border`
    /// over what the effect shows, whole.
    pub(crate) fn material(
        material: LoweredMaterial,
        read: EffectCaptureDraw,
        fill: BrushSource,
        border: LoweredBorder,
    ) -> Self {
        Self {
            effect: material.effect(),
            params: material.params,
            images: Span::default(),
            backdrop: Some(read),
            stage: Some(MaterialStage {
                fill: fill.gpu_fill(),
                fill_axis: fill.fill_axis(),
                border: border.normalized(),
                max_luminance: material.max_luminance(),
                min_luminance: material.min_luminance(),
            }),
            outside: None,
            opacity: 1.0,
            time: 0.0,
        }
    }

    /// The border a material's stage lays over its shape; none for an app's paint.
    pub(crate) fn border(&self) -> LoweredBorder {
        self.stage.map_or(LoweredBorder::NONE, |stage| stage.border)
    }
}

impl ClosedPaintPayload for ClosedEffect {
    /// This effect faded by `by`: its opacity, and a material's fill and border with it.
    #[inline]
    fn faded(self, by: f32) -> Self {
        Self {
            opacity: self.opacity * by,
            stage: self.stage.map(|stage| MaterialStage {
                fill: stage.fill.faded(by),
                border: stage.border.faded(by),
                ..stage
            }),
            ..self
        }
    }

    /// Paints nothing over a box of no extent or at no opacity. A triangle is never empty here:
    /// its covering rect exists only after transform, and its record and its placement drop a
    /// triangle of no area.
    #[inline]
    fn is_noop(&self, geometry: PlacedClosed) -> bool {
        let empty = match geometry {
            PlacedClosed::Box(placed) => placed.rect.is_paint_empty(),
            PlacedClosed::Triangle(_) => false,
        };
        empty || is_invisible(self.opacity)
    }

    fn paint(draw: DrawClosedPayload<Self>, sink: &mut impl PaintSink) {
        sink.effect(draw);
    }
}
