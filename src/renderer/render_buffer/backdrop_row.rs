//! One pane of a frame: what its capture reads and where its result comes from.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::shape::paint::blur_plan::BlurPlan;

/// What a pane's result is: the pane, the screen its capture reads, its blur, and the texels the
/// result covers. The pane ledger decides it before a frame encodes, and the composer's row
/// carries it to the backend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaneCapture {
    /// The pane across frames, which its cached blur keys on.
    pub(crate) key: PaneKey,
    /// The physical px the pane's capture reads, inside the surface.
    pub(crate) footprint: URect,
    pub(crate) plan: BlurPlan,
    /// The texels of the plan's last level the pane's result covers: its held capture's, or this
    /// frame's where it takes a new one.
    pub(crate) region: URect,
}

/// A backdrop the composer met: a pane, or a backdrop over the window's ground. Its effect
/// instance's `pane` is its index in
/// [`RenderBuffer::backdrops`](crate::renderer::render_buffer::RenderBuffer::backdrops).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropRow {
    /// The physical rect the composite reads, a read past it reading its edge: a backdrop
    /// effect's rect grown by its reach, a material's footprint, or the rect over the ground.
    pub(crate) reads: Rect,
    /// What a material's stage holds its result to and lays over it; `None` for an app's backdrop
    /// effect's shape, which reads it as it likes.
    pub(crate) look: Option<MaterialLook>,
    /// What it reads.
    pub(crate) read: BackdropRead,
}

/// What a backdrop reads: a pane's result, by the capture the pane ledger decided and where it
/// comes from this frame, or the window's ground, the clear, while transparency is reduced, which
/// captures nothing and holds no result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BackdropRead {
    Pane {
        capture: PaneCapture,
        source: PaneSource,
    },
    Ground,
}

impl BackdropRow {
    /// The pane's capture; `None` over the ground.
    pub(crate) const fn capture(&self) -> Option<&PaneCapture> {
        match &self.read {
            BackdropRead::Pane { capture, .. } => Some(capture),
            BackdropRead::Ground => None,
        }
    }
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
    /// Straight alpha, linear.
    pub(crate) tint: RgbaF16,
    /// Straight alpha, linear.
    pub(crate) stroke_color: RgbaF16,
    pub(crate) stroke_width: f32,
}

impl MaterialLook {
    /// A look that leaves the result as it is: no bound, no fill or border.
    pub(crate) const PLAIN: Self = Self {
        max_luminance: 65504.0,
        min_luminance: 0.0,
        tint: RgbaF16::TRANSPARENT,
        stroke_color: RgbaF16::TRANSPARENT,
        stroke_width: 0.0,
    };
}

/// Where a pane's result comes from on one frame, as the composer sets it from the pane's
/// [`PaneTicket`](crate::renderer::pane_captures::PaneTicket).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneSource {
    /// Its held result serves.
    Held,
    /// It captures the pixels painted before it out of the target and blurs them, which repaints
    /// its footprint on a partial frame. The main pass breaks after `groups[..barrier]` to copy
    /// the capture.
    Capture { barrier: u32 },
    /// Nothing is painted before it, so its backdrop is the clear, which a blur leaves as it is:
    /// it shows the clear and neither captures nor blurs.
    Clear,
    /// A later opaque quad over the whole surface folded into the clear and dropped the pane's
    /// draw. It shows nothing and neither captures nor blurs.
    Hidden,
}

impl PaneSource {
    /// Whether the pane takes a new result this frame, from a capture or the clear.
    pub(crate) const fn is_new(self) -> bool {
        matches!(self, Self::Capture { .. } | Self::Clear)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::geometry::urect::URect;
    use crate::primitives::identity::pane_key::PaneKey;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::renderer::render_buffer::backdrop_row::{
        BackdropRead, BackdropRow, MaterialLook, PaneCapture, PaneSource,
    };
    use crate::shape::paint::blur_plan::BlurPlan;

    impl BackdropRow {
        /// Pane `i` of the owner `"pane"`, reading `footprint` blurred by `plan`, which holds its
        /// result: no luminance bound, fill or border. Its region is `footprint` on the lattice.
        pub(crate) fn fixture(i: u32, footprint: URect, plan: BlurPlan) -> Self {
            Self {
                reads: footprint.into(),
                look: Some(MaterialLook::PLAIN),
                read: BackdropRead::Pane {
                    capture: PaneCapture {
                        key: PaneKey {
                            owner: WidgetId::from_hash("pane"),
                            shape: i,
                        },
                        footprint,
                        plan,
                        region: plan
                            .region(footprint, footprint.max())
                            .expect("a fixture's footprint paints"),
                    },
                    source: PaneSource::Held,
                },
            }
        }

        /// Where the pane's result comes from; `None` over the ground.
        pub(crate) const fn source(&self) -> Option<PaneSource> {
            match self.read {
                BackdropRead::Pane { source, .. } => Some(source),
                BackdropRead::Ground => None,
            }
        }

        /// This pane taking its result from `source`.
        pub(crate) fn with_source(mut self, source: PaneSource) -> Self {
            let BackdropRead::Pane { source: held, .. } = &mut self.read else {
                panic!("a pane fixture reads a pane");
            };
            *held = source;
            self
        }
    }
}
