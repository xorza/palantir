//! Tab and dock fixtures: the chip row's chrome, and the pane tree the
//! dock walks it onto.
//!
//! Both scenes are bare `fn`s over state the `Ui` holds, because
//! [`Harness::render_after_settle`] wants a `Copy` scene and a dock's
//! tree is anything but. That is also how a real application would host
//! one page's state, so the fixture is not bending the widget to be
//! photographable.

use glam::{UVec2, Vec2};
use palantir::golden::Tolerance;
use palantir::{
    Configure, DockDrop, DockOp, DockState, DockTabs, DockView, InternedStr, Panel, Sizing,
    SplitSide, TabBadge, TabItem, TabStrip, TabbedView, Text, Ui, WidgetId,
};

use crate::fixtures::DARK_BG;
use crate::goldens::assert_matches_golden;
use crate::harness::Harness;

/// A strip on its own: one selected chip wearing the accent cap, one
/// carrying an inked badge, and a close button on every one.
#[test]
fn tab_strip_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut Ui) {
        let items: Vec<TabItem> = [("curves", TabBadge::None), ("levels", TabBadge::On)]
            .into_iter()
            .chain([("export", TabBadge::None)])
            .enumerate()
            .map(|(i, (label, badge))| TabItem {
                badge,
                ..TabItem::new(i as u64, ui.intern(label))
            })
            .collect();
        Panel::vstack()
            .id_salt("strip-well")
            .size((Sizing::FILL, Sizing::HUG))
            .padding(12.0)
            .show(ui, |ui| {
                TabStrip::new(&items).id_salt("strip").selected(1).show(ui);
            });
    }
    let img = h.render_after_settle(2, UVec2::new(360, 76), 1.0, DARK_BG, scene);
    assert_matches_golden("tab_strip", &img, Tolerance::default());
}

/// A tabbed view: the same strip over a content area, so the selected
/// chip's bottom edge is seen dissolving into the page below it.
#[test]
fn tabbed_view_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut Ui) {
        ui.with_state::<usize, _>(WidgetId::from_hash("visual.page"), |ui, page| {
            *page = 1;
            TabbedView::new(page, &["Colour", "Geometry", "Metadata"])
                .id_salt("pages")
                .closable(false)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui, index| {
                    Panel::vstack()
                        .id_salt("page")
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(14.0)
                        .show(ui, |ui| {
                            Text::new(["Colour", "Geometry", "Metadata"][index])
                                .id_salt("page-title")
                                .show(ui);
                        });
                });
        });
    }
    let img = h.render_after_settle(2, UVec2::new(360, 140), 1.0, DARK_BG, scene);
    assert_matches_golden("tabbed_view", &img, Tolerance::default());
}

/// Three panes: the divider chrome, one strip per pane, and the dimmed
/// cap that marks the two panes not holding focus.
#[test]
fn dock_split_panes_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut Ui) {
        ui.with_state::<DockScene, _>(WidgetId::from_hash("visual.dock"), |ui, scene| {
            let DockScene { dock, panes } = scene;
            DockView::run(ui, dock, panes);
        });
    }
    let img = h.render_after_settle(2, UVec2::new(520, 220), 1.0, DARK_BG, scene);
    assert_matches_golden("dock_split_panes", &img, Tolerance::default());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Canvas,
    Layers,
    Console,
}

/// The dock and its viewer, as one `Ui` state row — see the module doc.
#[derive(Debug)]
struct DockScene {
    dock: DockState<Tab>,
    panes: Panes,
}

impl Default for DockScene {
    fn default() -> Self {
        let mut dock = DockState::new("visual.dock", Tab::Canvas);
        let primary = dock.primary().id;
        dock.find_or_insert(Tab::Layers, primary);
        dock.find_or_insert(Tab::Console, primary);
        dock.apply(DockOp::MoveTab {
            tab: Tab::Console,
            to: DockDrop::Split {
                group: primary,
                side: SplitSide::Right,
            },
        });
        dock.apply(DockOp::ActivateTab { tab: Tab::Canvas });
        Self { dock, panes: Panes }
    }
}

#[derive(Debug)]
struct Panes;

impl DockTabs for Panes {
    type Tab = Tab;

    fn title(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(match tab {
            Tab::Canvas => "canvas",
            Tab::Layers => "layers",
            Tab::Console => "console",
        })
    }

    fn content(&mut self, ui: &mut Ui, tab: Tab, _size: Option<Vec2>) {
        Panel::vstack()
            .id_salt(("pane", self.title_text(tab)))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(14.0)
            .show(ui, |ui| {
                Text::new(self.title_text(tab)).id_salt("body").show(ui);
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

impl Panes {
    /// The label without a `Ui` to intern it into — the pane body wants
    /// the same text for its id salt and its own line.
    fn title_text(&self, tab: Tab) -> &'static str {
        match tab {
            Tab::Canvas => "canvas",
            Tab::Layers => "layers",
            Tab::Console => "console",
        }
    }
}
