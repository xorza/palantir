//! One pane of a frame: what its capture reads and where it stands in paint order.

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
    /// The physical px the pane's capture reads: its visible rect grown by the plan's reach,
    /// inside the surface.
    pub(crate) footprint: URect,
    pub(crate) plan: BlurPlan,
    /// The groups painted before the pane, `groups[..barrier]`: the main pass breaks there to
    /// copy the capture out of the target.
    pub(crate) barrier: u32,
    /// The pane's lens with its lengths in physical px, if it is glass.
    pub(crate) lens: Option<Lens>,
    /// The highest relative luminance the pane shows under its tint.
    pub(crate) max_luminance: f32,
    /// Where the pane's result comes from this frame. Set by
    /// [`PaneCaptures::settle`](crate::renderer::pane_captures::PaneCaptures::settle), or
    /// [`PaneSource::Hidden`] by the composer.
    pub(crate) source: PaneSource,
}

/// Where a pane's result comes from on one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneSource {
    /// Its held result serves.
    Held,
    /// It captures the pixels painted before it out of the target and blurs them, which repaints
    /// its footprint on a partial frame.
    Capture,
    /// Nothing is painted before it, so its backdrop is the clear, which a blur leaves as it is:
    /// it shows the clear and neither captures nor blurs.
    Clear,
    /// A later opaque quad covers its quad, which the composer dropped: the occlusion pruner, or
    /// the clear fold. It shows nothing, captures nothing and holds no result, so it captures
    /// anew once it shows.
    Hidden,
}
