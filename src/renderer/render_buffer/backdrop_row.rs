//! One pane of a frame: what its capture reads and where its result comes from.

use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::paint::backdrop_filter::Lens;
use crate::shape::paint::blur_plan::BlurPlan;

/// A backdrop pane the composer met. Its quad's `fill_lut_row` is its index in
/// [`RenderBuffer::backdrops`](crate::renderer::render_buffer::RenderBuffer::backdrops).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackdropRow {
    /// The pane across frames, which its cached blur keys on.
    pub(crate) key: PaneKey,
    /// The physical px the pane's capture reads, inside the surface.
    pub(crate) footprint: URect,
    pub(crate) plan: BlurPlan,
    /// The texels of the plan's last level the pane's result covers: its held capture's, or this
    /// frame's where it takes a new one.
    pub(crate) region: URect,
    /// The pane's lens with its lengths in physical px, if it is glass.
    pub(crate) lens: Option<Lens>,
    /// The highest relative luminance the pane shows under its tint.
    pub(crate) max_luminance: f32,
    /// The lowest relative luminance the pane shows under its tint; 0 for none.
    pub(crate) min_luminance: f32,
    /// Where the pane's result comes from this frame.
    pub(crate) source: PaneSource,
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
    /// A later opaque quad covers its quad, which the composer dropped: the occlusion pruner, or
    /// the clear fold. It shows nothing and neither captures nor blurs.
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
    use crate::renderer::render_buffer::backdrop_row::{BackdropRow, PaneSource};
    use crate::shape::paint::blur_plan::BlurPlan;

    impl BackdropRow {
        /// Pane `i` of the owner `"pane"`, reading `footprint` blurred by `plan`, which holds its
        /// result: no lens and no luminance ceiling. Its region is `footprint` on the lattice.
        pub(crate) fn fixture(i: u32, footprint: URect, plan: BlurPlan) -> Self {
            Self {
                key: PaneKey {
                    owner: WidgetId::from_hash("pane"),
                    shape: i,
                },
                footprint,
                plan,
                region: plan
                    .region(footprint, footprint.max())
                    .expect("a fixture's footprint paints"),
                lens: None,
                max_luminance: 65504.0,
                min_luminance: 0.0,
                source: PaneSource::Held,
            }
        }
    }
}
