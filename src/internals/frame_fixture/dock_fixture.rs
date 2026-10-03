//! A settled dock as a recordable scene, at the scale a real editor runs
//! one.

use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::text::interned_str::InternedStr;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::dock::dock_op::{DockDrop, DockOp};
use crate::widgets::dock::dock_state::DockState;
use crate::widgets::dock::dock_tabs::DockTabs;
use crate::widgets::dock::dock_view::DockView;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::panel::Panel;
use crate::widgets::tabs::tab_item::TabBadge;
use crate::widgets::text::Text;

/// Three panes and two dividers: the pinned canvas beside a split-off
/// console, with an output pane under the console, and five tabs across
/// the three strips. Each pane's body is one line of its title.
///
/// One tree for the allocation gates and the visual suite alike, as
/// [`FrameFixture`](crate::internals::frame_fixture::FrameFixture) is for
/// the frame workload, so neither keeps a stand-in of its own.
#[derive(Debug)]
pub struct DockFixture {
    dock: DockState<Tab>,
    panes: Panes,
    ops: Vec<DockOp<Tab>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Canvas,
    Layers,
    History,
    Console,
    Output,
}

impl Tab {
    const fn title(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::Layers => "layers",
            Self::History => "history",
            Self::Console => "console",
            Self::Output => "output",
        }
    }
}

impl Default for DockFixture {
    fn default() -> Self {
        let mut dock = DockState::new("frame_fixture.dock", Tab::Canvas);
        let primary = dock.primary().id;
        for tab in [Tab::Layers, Tab::History, Tab::Console, Tab::Output] {
            dock.find_or_insert(tab, primary);
        }
        dock.apply(DockOp::MoveTab {
            tab: Tab::Console,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Right,
            },
        });
        let right = dock.focused();
        dock.apply(DockOp::MoveTab {
            tab: Tab::Output,
            to: DockDrop::Split {
                group: right,
                side: SplitSide::Bottom,
            },
        });
        dock.apply(DockOp::ActivateTab { tab: Tab::Canvas });
        Self {
            dock,
            panes: Panes,
            ops: Vec::new(),
        }
    }
}

impl DockFixture {
    /// Record one frame through [`DockView::run`], the one-call surface.
    pub fn record(&mut self, ui: &mut Ui) {
        DockView::run(ui, &mut self.dock, &mut self.panes);
    }

    /// Record one frame through the two-call surface: scan last frame's
    /// responses into the reused op buffer, apply them, then show.
    pub fn record_scanned(&mut self, ui: &mut Ui) {
        self.ops.clear();
        DockView::scan(&self.dock, ui, &mut self.ops);
        for op in self.ops.drain(..) {
            self.dock.apply(op);
        }
        DockView::new(&self.dock, &mut self.ops)
            .min_pane(120.0)
            .show(ui, &mut self.panes);
        for op in self.ops.drain(..) {
            self.dock.apply(op);
        }
    }
}

#[derive(Debug)]
struct Panes;

impl DockTabs for Panes {
    type Tab = Tab;

    fn title(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(tab.title())
    }

    fn content(&mut self, ui: &mut Ui, tab: Tab, _size: Option<Size>) {
        Panel::vstack()
            .id_salt(("pane", tab.title()))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(14.0)
            .show(ui, |ui| {
                Text::new(tab.title()).id_salt("body").show(ui);
            });
    }

    fn closable(&mut self, tab: Tab) -> bool {
        tab != Tab::Canvas
    }

    fn badge(&mut self, tab: Tab) -> TabBadge {
        match tab {
            Tab::Canvas => TabBadge::On,
            _ => TabBadge::None,
        }
    }
}
