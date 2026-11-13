//! One backdrop of a frame: what its capture reads and where its result comes from.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::shape::paint::blur_plan::BlurPlan;

/// What a backdrop's result is: the backdrop, the screen its capture reads, its blur, and the
/// texels the result covers. The backdrop ledger decides it before a frame encodes, and the
/// composer's row carries it to the backend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropCapture {
    /// The backdrop across frames, which its cached blur keys on.
    pub(crate) key: BackdropKey,
    /// The physical px the backdrop's capture reads, inside the surface.
    pub(crate) footprint: URect,
    pub(crate) plan: BlurPlan,
    /// The texels of the plan's last level the backdrop's result covers: its held capture's, or
    /// this frame's where it takes a new one.
    pub(crate) region: URect,
}

/// A backdrop the composer met: its captured result, by the capture the backdrop ledger decided
/// and where it comes from this frame. Its effect instance's `backdrop` is its index in
/// [`RenderBuffer::backdrops`](crate::renderer::render_buffer::RenderBuffer::backdrops).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropRow {
    /// The physical rect the composite reads, a read past it reading its edge: a backdrop
    /// effect's rect grown by its reach, or a material's footprint.
    pub(crate) reads: Rect,
    /// What a material's stage holds its result to and lays over it; `None` for an app's backdrop
    /// effect's paint, which reads it as it likes.
    pub(crate) look: Option<MaterialLook>,
    pub(crate) capture: BackdropCapture,
    pub(crate) source: BackdropSource,
}

/// What a material's stage does to what its effect shows, as the composer hands it on, lengths in
/// physical px: the luminance it holds it within, and the solid fill and the border over it, each
/// faded by a paint animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MaterialLook {
    /// The highest relative luminance the material shows under its fill.
    pub(crate) max_luminance: f32,
    /// The lowest relative luminance the material shows under its fill; 0 for none.
    pub(crate) min_luminance: f32,
    /// The box's fill: a straight linear color, or the multiplier on the gradient its lanes name.
    pub(crate) fill: GpuFill,
    /// The gradient's geometry in the box's 0..1 frame; zero for a solid.
    pub(crate) fill_axis: FillAxis,
    /// Straight alpha, linear.
    pub(crate) border_color: RgbaF16,
    pub(crate) border_width: f32,
}

impl MaterialLook {
    /// A look that leaves the result as it is: no bound, no fill or border.
    pub(crate) const PLAIN: Self = Self {
        max_luminance: 65504.0,
        min_luminance: 0.0,
        fill: GpuFill {
            color: RgbaF16::TRANSPARENT,
            kind: FillKind::SOLID,
            lut_row: LutRow::FALLBACK,
        },
        fill_axis: FillAxis::ZERO,
        border_color: RgbaF16::TRANSPARENT,
        border_width: 0.0,
    };
}

/// Where a backdrop's result comes from on one frame, as the composer sets it from the backdrop's
/// [`BackdropTicket`](crate::renderer::backdrop_captures::BackdropTicket).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackdropSource {
    /// Its held result serves.
    Held,
    /// It captures the pixels painted before it out of the target and blurs them, which repaints
    /// its footprint on a partial frame. The main pass breaks after `runs[..barrier]` to copy
    /// the capture.
    Capture { barrier: u32 },
    /// Nothing is painted before it, so its backdrop is the clear, which a blur leaves as it is:
    /// it shows the clear and neither captures nor blurs.
    Clear,
    /// A later opaque quad over the whole surface folded into the clear and dropped the backdrop's
    /// draw. It shows nothing and neither captures nor blurs.
    Hidden,
}

impl BackdropSource {
    /// Whether the backdrop takes a new result this frame, from a capture or the clear.
    pub(crate) const fn is_new(self) -> bool {
        matches!(self, Self::Capture { .. } | Self::Clear)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::geometry::urect::URect;
    use crate::primitives::identity::backdrop_key::BackdropKey;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::renderer::render_buffer::backdrop_row::{
        BackdropCapture, BackdropRow, BackdropSource, MaterialLook,
    };
    use crate::shape::paint::blur_plan::BlurPlan;

    impl BackdropRow {
        /// Backdrop `i` of the owner `"backdrop"`, reading `footprint` blurred by `plan`, which
        /// holds its result: no luminance bound, fill or border. Its region is `footprint` on the
        /// lattice.
        pub(crate) fn fixture(i: u32, footprint: URect, plan: BlurPlan) -> Self {
            Self {
                reads: footprint.into(),
                look: Some(MaterialLook::PLAIN),
                capture: BackdropCapture {
                    key: BackdropKey {
                        owner: WidgetId::from_hash("backdrop"),
                        shape: i,
                    },
                    footprint,
                    plan,
                    region: plan
                        .region(footprint, footprint.max())
                        .expect("a fixture's footprint paints"),
                },
                source: BackdropSource::Held,
            }
        }

        /// This backdrop taking its result from `source`.
        pub(crate) const fn with_source(mut self, source: BackdropSource) -> Self {
            self.source = source;
            self
        }
    }
}
