//! [`FrameEngines`] — the incremental machinery one window's frame loop
//! drives, held by the driver rather than by the recorder it drives.

use crate::layout::engine::LayoutEngine;
use crate::scene::cascade::engine::CascadeEngine;
use crate::scene::damage::DamageEngine;
use crate::ui::resources::UiResources;

/// The three engines [`FrameCycle`](crate::ui::frame_cycle::FrameCycle) runs
/// over a [`Ui`](crate::Ui), and the retained caches each needs to run
/// incrementally: the measure cache, the previous cascade's rows, last frame's
/// paint snapshot.
///
/// **Owned by the frame driver, not by `Ui`.** Nothing outside `FrameCycle`
/// runs an engine, so keeping them on the recorder only made three subsystems
/// reachable from every widget in the crate. The pass signatures already drew
/// this line — [`LayoutEngine::run`] takes `&Forest` and writes `&mut Layout`,
/// [`CascadeEngine::run`] takes `&Forest`/`&Layout` and writes `&mut Cascade`
/// — so the engine was always separate from the table it produces; this makes
/// the ownership say so too.
///
/// Per-window like the `Ui` it pairs with: `WindowDriver` holds one beside its
/// recorder, and so does `UiHarness`.
///
/// The fields are `pub(crate)` and that is not the hole it looks like: the
/// boundary is the *reachability of a value*, not the modifier on a field.
/// Only two places own one — `WindowDriver`, where the field is private to
/// `crate::host::window_driver`, and `UiHarness`, which does not exist outside
/// `cfg(any(test, feature = "internals"))`. Production code has no path to a
/// live `FrameEngines`, so widget code cannot read its own window's pipeline
/// state however these fields are spelled; the in-crate damage and layout
/// suites, which assert on cache and counter internals, reach theirs off the
/// harness.
#[derive(Debug)]
pub(crate) struct FrameEngines {
    /// Measure/arrange, plus the measure cache and the `TextSystem` whose
    /// clock the glyph atlas ages on.
    pub(crate) layout: LayoutEngine,
    pub(crate) cascade: CascadeEngine,
    /// Retains the previous frame's paint snapshot, which is what makes the
    /// damage diff incremental — so this outlives a frame, unlike scratch.
    pub(crate) damage: DamageEngine,
}

impl FrameEngines {
    /// Build the engines for a recorder constructed from `resources`. Takes
    /// the bundle rather than a bare shaper so the one clone the layout engine
    /// needs is spelled here, next to the field that keeps it.
    pub(crate) fn new(resources: &UiResources) -> Self {
        Self {
            layout: LayoutEngine::new(resources.text.clone()),
            cascade: CascadeEngine::default(),
            damage: DamageEngine::default(),
        }
    }
}
