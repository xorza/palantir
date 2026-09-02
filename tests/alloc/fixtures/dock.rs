//! A settled dock frame, at the scale a real editor runs one: three
//! panes, two dividers, several chips per strip.
//!
//! The claim under audit is the composition, not any one widget. A dock
//! frame runs a scan over last frame's responses, a recursive walk onto
//! two `Splitter`s, a `TabStrip` per pane whose items are rebuilt every
//! frame from the application's own answers, and a per-chip context menu
//! that stays closed — every one of them a place a fresh `Vec` would be
//! easy to write and invisible to look at.

use glam::Vec2;
use palantir::{
    Configure, DockDrop, DockOp, DockState, DockTabs, DockView, InternedStr, Panel, Sizing,
    SplitSide, TabBadge, Text, Ui,
};

use crate::harness::Audit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Canvas,
    Layers,
    History,
    Console,
    Output,
}

const OPENED: [Tab; 4] = [Tab::Layers, Tab::History, Tab::Console, Tab::Output];

#[derive(Debug)]
struct Panes;

impl DockTabs for Panes {
    type Tab = Tab;

    fn title(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(match tab {
            Tab::Canvas => "canvas",
            Tab::Layers => "layers",
            Tab::History => "history",
            Tab::Console => "console",
            Tab::Output => "output",
        })
    }

    fn content(&mut self, ui: &mut Ui, _tab: Tab, _size: Option<Vec2>) {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(8.0)
            .show(ui, |ui| {
                Text::new("pane body").auto_id().show(ui);
            });
    }

    fn closable(&mut self, tab: Tab) -> bool {
        tab != Tab::Canvas
    }

    fn badge(&mut self, tab: Tab) -> TabBadge {
        match tab {
            Tab::Canvas => TabBadge::Idle,
            _ => TabBadge::None,
        }
    }
}

/// Three panes: the pinned canvas beside a split-off console, with an
/// output pane under it.
fn seeded() -> DockState<Tab> {
    let mut dock = DockState::new("alloc.dock", Tab::Canvas);
    let primary = dock.primary().id;
    for tab in OPENED {
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
    dock
}

#[test]
fn settled_dock_frame_alloc_free() {
    let mut dock = seeded();
    let mut panes = Panes;
    Audit::new().run(move |ui| {
        DockView::run(ui, &mut dock, &mut panes);
    });
}

/// The two-call surface pays no more than the one-call one: the caller's
/// own op buffer is the same reused `Vec` `run` keeps internally.
#[test]
fn scan_then_record_is_alloc_free_too() {
    let mut dock = seeded();
    let mut panes = Panes;
    let mut ops: Vec<DockOp<Tab>> = Vec::new();
    Audit::new().run(move |ui| {
        ops.clear();
        dock.scan(ui, &mut ops);
        for op in ops.drain(..) {
            dock.apply(op);
        }
        DockView::new(&dock, &mut ops)
            .min_pane(120.0)
            .show(ui, &mut panes);
        for op in ops.drain(..) {
            dock.apply(op);
        }
    });
}
