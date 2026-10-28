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
    /// Whether the pane captures this frame, which repaints its footprint; else its held result
    /// serves. Set by [`PaneCaptures::settle`](crate::renderer::pane_captures::PaneCaptures::settle).
    pub(crate) capture: bool,
}
