//! A settled dock as a recordable scene, at a real editor's scale.

use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::text::interned_str::InternedStr;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::dock::dock_operation::{DockDrop, DockOperation};
use crate::widgets::dock::dock_state::DockState;
use crate::widgets::dock::dock_tabs::DockTabs;
use crate::widgets::dock::dock_view::DockView;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::panel::Panel;
use crate::widgets::tabs::tab_item::TabBadge;
use crate::widgets::text::Text;

/// Three panes and two dividers (canvas beside a console, an output pane under it) and five tabs across three strips; each body is one line of its title. Shared by the allocation gates and visual suite.
#[derive(Debug)]
pub struct DockFixture {
    dock: DockState<Tab>,
    panes: Panes,
    operations: Vec<DockOperation<Tab>>,
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
        dock.apply(DockOperation::MoveTab {
            tab: Tab::Console,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Right,
            },
        });
        let right = dock.focused();
        dock.apply(DockOperation::MoveTab {
            tab: Tab::Output,
            to: DockDrop::Split {
                group: right,
                side: SplitSide::Bottom,
            },
        });
        dock.apply(DockOperation::ActivateTab { tab: Tab::Canvas });
        Self {
            dock,
            panes: Panes,
            operations: Vec::new(),
        }
    }
}

impl DockFixture {
    /// Record one frame through [`DockView::run`], the one-call surface.
    pub fn record(&mut self, ui: &mut Ui) {
        DockView::run(ui, &mut self.dock, &mut self.panes);
    }

    /// Record one frame through the two-call surface: scan responses into the reused operation buffer, apply, then show.
    pub fn record_scanned(&mut self, ui: &mut Ui) {
        self.operations.clear();
        DockView::scan(ui, &self.dock, &mut self.operations);
        for operation in self.operations.drain(..) {
            self.dock.apply(operation);
        }
        DockView::new(&self.dock, &mut self.operations)
            .min_pane(120.0)
            .show(ui, &mut self.panes);
        for operation in self.operations.drain(..) {
            self.dock.apply(operation);
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
