//! [`FrameEngines`]: the incremental machinery one window's frame loop drives, held by the driver, not the recorder.

use crate::cascade::engine::CascadeEngine;
use crate::damage::engine::DamageEngine;
use crate::layout::engine::LayoutEngine;
use crate::ui::resources::UiResources;

/// The three engines [`FrameCycle`](crate::ui::frame_cycle::FrameCycle) runs over a [`Ui`](crate::Ui), with the
/// retained caches each needs to run incrementally (measure cache, previous cascade, last paint snapshot). Owned by
/// the frame driver, not `Ui`: nothing outside `FrameCycle` runs an engine, and on the recorder they would be
/// reachable from every widget. Per-window: `WindowDriver` and `UiHarness` each hold one; production code has no
/// path to a live `FrameEngines`, so its `pub(crate)` fields do not open a hole.
#[derive(Debug)]
pub(crate) struct FrameEngines {
    /// Measure/arrange, plus the measure cache and the `TextSystem` whose clock ages the glyph atlas.
    pub(crate) layout: LayoutEngine,
    pub(crate) cascade: CascadeEngine,
    /// Retains the previous frame's paint snapshot, making the damage diff incremental.
    pub(crate) damage: DamageEngine,
}

impl FrameEngines {
    /// Builds the engines for a recorder made from `resources`.
    pub(crate) fn new(resources: &UiResources) -> Self {
        Self {
            layout: LayoutEngine::new(resources.text().clone()),
            cascade: CascadeEngine::default(),
            damage: DamageEngine::default(),
        }
    }
}
