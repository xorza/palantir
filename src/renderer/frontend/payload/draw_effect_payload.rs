//! One effect draw: an app's `EffectPaint`, or a box's material, which palantir's stage wraps.

use crate::common::span::Span;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::paint::capture_params::CaptureReach;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;

/// An effect over a box or a triangle in the logical frame: an app's paint, or a box's material,
/// which carries its stage. `params` indexes the window's
/// [`RecordStore`](crate::shape::record_store::RecordStore) parameter words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawEffectPayload {
    /// The shape in the logical frame: a material's box may cover its outside, an app's paint
    /// covers its inside.
    pub(crate) geometry: PlacedClosed,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
    pub(crate) effect: EffectId,
    pub(crate) params: Span,
    /// Its images, indexed into the record store's image column.
    pub(crate) images: Span,
    /// A backdrop effect's backdrop; `None` for a color effect.
    pub(crate) backdrop: Option<EffectCaptureDraw>,
    /// A material's stage around what the effect shows; `None` for an app's paint.
    pub(crate) stage: Option<MaterialStage>,
    /// A paint animation's fade, which scales the coverage, a material's stage alike.
    pub(crate) opacity: f32,
    /// Seconds into the paint animation's pass, which the effect reads; 0 when unanimated.
    pub(crate) time: f32,
}

/// What a backdrop effect's draw reads: what is painted under it, by the ticket the backdrop
/// ledger wrote for it, up to its reach past its rect; or the window's ground, while transparency
/// is reduced, under its rect alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EffectCaptureDraw {
    Capture {
        ticket: BackdropTicket,
        reach: CaptureReach,
    },
    Ground,
}

/// What palantir lays around a material's effect: the luminance bounds it holds what the effect
/// shows to, then the box's fill, a color or a gradient, and its border over the whole shape.
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

impl DrawEffectPayload {
    /// A material over `geometry`, reading `read`: its effect under its stage, the box's `fill`
    /// and `border` over what the effect shows, unturned and whole.
    pub(crate) fn material(
        material: LoweredMaterial,
        read: EffectCaptureDraw,
        geometry: PlacedClosed,
        fill: BrushSource,
        border: LoweredBorder,
    ) -> Self {
        Self {
            geometry,
            spin: None,
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
            opacity: 1.0,
            time: 0.0,
        }
    }

    /// This draw turned by `spin`.
    pub(crate) const fn turned(mut self, spin: Option<Spin>) -> Self {
        self.spin = spin;
        self
    }

    /// The shape as a quad packs it, which the effect's instance takes its rect, shape, turn and
    /// border width from, so it lies on the pixels a quad of it does.
    pub(crate) fn quad(&self) -> DrawQuadPayload {
        DrawQuadPayload {
            geometry: self.geometry,
            spin: self.spin,
            fill: GpuFill {
                color: RgbaF16::TRANSPARENT,
                kind: FillKind::SOLID,
                lut_row: LutRow::FALLBACK,
            },
            border: self.stage.map_or(LoweredBorder::NONE, |stage| stage.border),
            fill_axis: FillAxis::ZERO,
        }
    }
}

impl PaintPayload for DrawEffectPayload {
    /// This draw faded by `by`: its opacity, and a material's fill and border with it.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
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

    /// Paints nothing over an empty box or at no opacity. A triangle is never empty here: its
    /// covering rect exists only after transform, and its record and its placement drop a
    /// triangle of no area.
    #[inline]
    fn is_noop(&self) -> bool {
        let empty = match self.geometry {
            PlacedClosed::Box { placed, .. } => placed.rect.is_paint_empty(),
            PlacedClosed::Triangle(_) => false,
        };
        empty || is_invisible(self.opacity)
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.effect(self);
    }
}
