//! A live dock: drag a chip to another pane's edge to split, into its strip to join, onto a divider to resize.

use crate::support;
use crate::support::{body_style, note_style, well_bg};
use palantir::{
    Button, Configure, DockDrop, DockOperation, DockState, DockTabMenu, DockTabs, DockView,
    InternedStr, MenuItem, Panel, Size, Sizing, SplitSide, TabBadge, Text, Ui, WidgetId, fmt,
};

/// The showcase's tab key: a small `Copy` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Canvas,
    Layers,
    History,
    Console,
}

const OPENABLE: [Tab; 3] = [Tab::Layers, Tab::History, Tab::Console];

impl Tab {
    const fn label(self) -> &'static str {
        match self {
            Tab::Canvas => "canvas",
            Tab::Layers => "layers",
            Tab::History => "history",
            Tab::Console => "console",
        }
    }

    const fn blurb(self) -> &'static str {
        match self {
            Tab::Canvas => "The pinned tab. It refuses to close, so the tree is never empty.",
            Tab::Layers => "Drag this chip onto another pane's edge to split it.",
            Tab::History => "Drop a chip into a strip to join that pane instead.",
            Tab::Console => "Right-click a chip for the split menu.",
        }
    }
}

#[derive(Debug)]
struct State {
    dock: DockState<Tab>,
    /// The frame's operation sink, reused so the page allocates once.
    operations: Vec<DockOperation<Tab>>,
}

impl Default for State {
    fn default() -> Self {
        let mut dock = DockState::new("showcase.dock", Tab::Canvas);
        let primary = dock.primary().id;
        for tab in OPENABLE {
            dock.find_or_insert(tab, primary);
        }
        dock.apply(DockOperation::MoveTab {
            tab: Tab::Console,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Bottom,
            },
        });
        dock.apply(DockOperation::ActivateTab { tab: Tab::Canvas });
        Self {
            dock,
            operations: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct Panes;

impl DockTabs for Panes {
    type Tab = Tab;

    fn title(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(tab.label())
    }

    fn content(&mut self, ui: &mut Ui, tab: Tab, size: Option<Size>) {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(14.0)
            .gap(6.0)
            .background(well_bg())
            .show(ui, |ui| {
                Text::new(tab.label()).style(&body_style()).show(ui);
                Text::new(tab.blurb()).style(&note_style()).show(ui);
                let measured = match size {
                    Some(s) => fmt!(ui, "content area {:.0} x {:.0}", s.w, s.h),
                    None => ui.intern("content area not laid out yet"),
                };
                Text::new(measured).style(&note_style()).show(ui);
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

    fn tab_menu(&mut self, ui: &mut Ui, menu: DockTabMenu<'_, Tab>) {
        let mut side = None;
        if MenuItem::new("Split right").show(ui, menu.close).clicked() {
            side = Some(SplitSide::Right);
        }
        if MenuItem::new("Split down").show(ui, menu.close).clicked() {
            side = Some(SplitSide::Bottom);
        }
        if let Some(side) = side {
            menu.operations.push(DockOperation::MoveTab {
                tab: menu.tab,
                to: DockDrop::Split {
                    group: menu.group,
                    side,
                },
            });
        }
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::dock::state");
    ui.with_state::<State, _>(state_id, |ui, s| {
        reopen_row(ui, s);
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let mut panes = Panes;
                // The scan settles the arrangement before the record walks it, so a click draws on the frame it lands.
                s.operations.clear();
                DockView::scan(ui, &s.dock, &mut s.operations);
                for operation in s.operations.drain(..) {
                    s.dock.apply(operation);
                }
                DockView::new(&s.dock, &mut s.operations)
                    .min_pane(140.0)
                    .show(ui, &mut panes);
                for operation in s.operations.drain(..) {
                    s.dock.apply(operation);
                }
            });
    });
}

/// Re-open closed tabs so the demo can't be emptied down to the pinned pane.
fn reopen_row(ui: &mut Ui, s: &mut State) {
    let all_open = OPENABLE.iter().all(|&t| s.dock.find_tab(t).is_some());
    let line = if all_open {
        "every tab is open — drag a chip onto a pane edge to split it"
    } else {
        "closed tabs reopen in the focused pane:"
    };
    support::row(ui, |ui| {
        Text::new(line).style(&note_style()).show(ui);
        for tab in OPENABLE {
            if s.dock.find_tab(tab).is_some() {
                continue;
            }
            if Button::new()
                .id_salt(tab.label())
                .label(tab.label())
                .show(ui)
                .clicked()
            {
                s.dock.apply(DockOperation::OpenTab { tab });
            }
        }
    });
}
