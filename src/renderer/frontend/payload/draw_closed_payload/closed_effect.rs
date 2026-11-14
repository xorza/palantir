//! [`ClosedEffect`]: an effect over a closed shape, an app's `EffectPaint` or a box's material,
//! which palantir's stage wraps.

use crate::common::span::Span;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::domain::is_invisible;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::draw_closed_payload::{
    ClosedPaintPayload, DrawClosedPayload,
};
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
    /// What it reads besides its own inputs, by its kind.
    pub(crate) read: EffectRead,
    /// A paint animation's fade, which scales the coverage, a material's stage alike.
    pub(crate) opacity: f32,
    /// Seconds into the paint animation's pass, which the effect reads; 0 when unanimated.
    pub(crate) time: f32,
}

/// What an effect's draw reads besides its own inputs, one case a kind, so a stage never draws
/// without the backdrop it composes against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EffectRead {
    /// A color effect, over its shape, or a halo `outside` it alone, logical px past it.
    Color { outside: Option<f32> },
    /// An app's backdrop effect, over what is painted under it.
    Backdrop { capture: EffectCaptureDraw },
    /// A material: a backdrop effect under the stage palantir lays around what it shows.
    Material {
        capture: EffectCaptureDraw,
        stage: MaterialStage,
    },
}

/// What a backdrop effect's draw reads: what is painted under it, by the ticket the backdrop
/// ledger wrote for it, up to its reach past its rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectCaptureDraw {
    pub(crate) ticket: BackdropTicket,
    pub(crate) reach: CaptureReach,
}

/// What palantir lays around a material's effect: the luminance bounds it holds what the effect
/// shows to, then the box's fill and border over the whole shape, which the material draws in its
/// own draw (`src/shape/AGENTS.md`, "A box's fill reaches under its border, a material under
/// both, in one draw"); the two routes share one composite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MaterialStage {
    pub(crate) paint: ClosedFill,
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
            read: EffectRead::Material {
                capture: read,
                stage: MaterialStage {
                    paint: ClosedFill::new(fill, border),
                    max_luminance: material.max_luminance(),
                    min_luminance: material.min_luminance(),
                },
            },
            opacity: 1.0,
            time: 0.0,
        }
    }

    /// The border a material's stage lays over its shape; none for an app's paint.
    pub(crate) const fn border(&self) -> LoweredBorder {
        match self.read {
            EffectRead::Material { stage, .. } => stage.paint.border,
            EffectRead::Color { .. } | EffectRead::Backdrop { .. } => LoweredBorder::NONE,
        }
    }
}

impl ClosedPaintPayload for ClosedEffect {
    /// This effect faded by `by`: its opacity, and a material's fill and border with it.
    #[inline]
    fn faded(self, by: f32) -> Self {
        let read = match self.read {
            EffectRead::Material { capture, stage } => EffectRead::Material {
                capture,
                stage: MaterialStage {
                    paint: stage.paint.faded(by),
                    ..stage
                },
            },
            read @ (EffectRead::Color { .. } | EffectRead::Backdrop { .. }) => read,
        };
        Self {
            opacity: self.opacity * by,
            read,
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
