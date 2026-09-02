//! A live dock: drag a chip onto another pane's edge to split it, into
//! its strip to join, and onto a divider to resize.

use crate::support;
use crate::support::{body_style, note_style, well_bg};
use glam::Vec2;
use palantir::{
    Button, Configure, DockDrop, DockOp, DockState, DockTabMenu, DockTabs, DockView, InternedStr,
    MenuItem, Panel, Sizing, SplitSide, TabBadge, Text, Ui, WidgetId, fmt,
};

/// The showcase's own tab key: a small `Copy` value, which is all the
/// dock ever stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Canvas,
    Layers,
    History,
    Console,
}

const OPENABLE: [Tab; 3] = [Tab::Layers, Tab::History, Tab::Console];

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Canvas => "canvas",
            Tab::Layers => "layers",
            Tab::History => "history",
            Tab::Console => "console",
        }
    }

    fn blurb(self) -> &'static str {
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
    /// The frame's op sink, cleared and refilled by the two calls
    /// below. A field rather than a local so the page allocates once
    /// rather than once a frame.
    ops: Vec<DockOp<Tab>>,
}

impl Default for State {
    fn default() -> Self {
        let mut dock = DockState::new("showcase.dock", Tab::Canvas);
        let primary = dock.primary().id;
        for tab in OPENABLE {
            dock.find_or_insert(tab, primary);
        }
        dock.apply(DockOp::MoveTab {
            tab: Tab::Console,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Bottom,
            },
        });
        dock.apply(DockOp::ActivateTab { tab: Tab::Canvas });
        Self {
            dock,
            ops: Vec::new(),
        }
    }
}

/// The showcase's viewer: every tab is a caption and a blurb.
#[derive(Debug)]
struct Panes;

impl DockTabs for Panes {
    type Tab = Tab;

    fn title(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(tab.label())
    }

    fn content(&mut self, ui: &mut Ui, tab: Tab, size: Option<Vec2>) {
        Panel::vstack()
            .id_salt(("pane", tab.label()))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(14.0)
            .gap(6.0)
            .background(well_bg())
            .show(ui, |ui| {
                Text::new(tab.label())
                    .id_salt("title")
                    .style(&body_style())
                    .show(ui);
                Text::new(tab.blurb())
                    .id_salt("blurb")
                    .style(&note_style())
                    .show(ui);
                let measured = match size {
                    Some(s) => fmt!(ui, "content area {:.0} x {:.0}", s.x, s.y),
                    None => ui.intern("content area not laid out yet"),
                };
                Text::new(measured)
                    .id_salt("size")
                    .style(&note_style())
                    .show(ui);
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
        if MenuItem::new("Split right")
            .show(ui, menu.close)
            .left
            .clicked()
        {
            side = Some(SplitSide::Right);
        }
        if MenuItem::new("Split down")
            .show(ui, menu.close)
            .left
            .clicked()
        {
            side = Some(SplitSide::Bottom);
        }
        if let Some(side) = side {
            menu.ops.push(DockOp::MoveTab {
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
            .id_salt("dock-well")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let mut panes = Panes;
                // The two-call surface, spelled out: the scan settles the
                // arrangement before the record walks it, so a click draws
                // on the frame it lands.
                s.ops.clear();
                s.dock.scan(ui, &mut s.ops);
                for op in s.ops.drain(..) {
                    s.dock.apply(op);
                }
                DockView::new(&s.dock, &mut s.ops)
                    .id_salt("dock")
                    .min_pane(140.0)
                    .show(ui, &mut panes);
                for op in s.ops.drain(..) {
                    s.dock.apply(op);
                }
            });
    });
}

/// Buttons that re-open whichever tabs are closed, so the demo cannot be
/// emptied down to the pinned pane and left there.
fn reopen_row(ui: &mut Ui, s: &mut State) {
    let closed: Vec<Tab> = OPENABLE
        .into_iter()
        .filter(|t| s.dock.find_tab(*t).is_none())
        .collect();
    let line = if closed.is_empty() {
        ui.intern("every tab is open — drag a chip onto a pane edge to split it")
    } else {
        ui.intern("closed tabs re-open in the focused pane:")
    };
    support::row(ui, |ui| {
        Text::new(line)
            .id_salt("dock-note")
            .style(&note_style())
            .show(ui);
        for tab in closed {
            if Button::new()
                .id_salt(("reopen", tab.label()))
                .label(tab.label())
                .show(ui)
                .left
                .clicked()
            {
                s.dock.apply(DockOp::OpenTab { tab });
            }
        }
    });
}
