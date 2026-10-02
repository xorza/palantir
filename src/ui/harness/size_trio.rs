//! The arranged sizes a widget's "explicit size beats the default" test
//! reads.

use crate::Ui;
use crate::layout::types::sizing::{SizeSpec, Sizing};
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// Three copies of one widget in a 400×300 `FILL` column: one sized by
/// the caller, one sized `HUG × HUG`, and one left at its default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SizeTrio {
    pub(crate) sized: Size,
    pub(crate) hug: Size,
    pub(crate) default: Size,
}

impl SizeTrio {
    /// Arrange the three copies `show` records — `explicit`, `HUG × HUG`,
    /// then `None` for the default — and read their sizes.
    pub(crate) fn of(
        explicit: impl Into<SizeSpec>,
        mut show: impl FnMut(&mut Ui, Option<SizeSpec>) -> NodeId,
    ) -> Self {
        let explicit = explicit.into();
        let mut h = UiHarness::new(UVec2::new(400, 300));
        let nodes = h.frame_value(|ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    [
                        Some(explicit),
                        Some(SizeSpec::new(Sizing::HUG, Sizing::HUG)),
                        None,
                    ]
                    .map(|size| show(ui, size))
                })
                .inner
        });
        let size = |node: NodeId| h.ui.arranged_rect(Layer::Main, node).size;
        Self {
            sized: size(nodes[0]),
            hug: size(nodes[1]),
            default: size(nodes[2]),
        }
    }
}
