//! Tab and dock fixtures: the chip row's chrome, and the pane tree the
//! dock walks it onto.
//!
//! Both scenes are bare `fn`s over state the `Ui` holds, which is how a
//! real application would host one page's state, so the fixture is not
//! bending the widget to be photographable.

use glam::UVec2;
use palantir::internals::frame_fixture::dock_fixture::DockFixture;
use palantir::{
    Configure, Panel, Sizing, TabBadge, TabItem, TabStrip, TabbedView, Text, Ui, WidgetId,
};

use crate::golden_name::GoldenName;
use crate::goldens::assert_matches_golden;
use crate::harness::Harness;

/// A strip on its own: one selected chip wearing the accent cap, one
/// carrying an inked badge, and a close button on every one.
#[test]
fn tab_strip_matches_golden() {
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

    let mut h = Harness::new();
    let img = h.size(UVec2::new(360, 76)).settled_frame(2, scene).image;
    assert_matches_golden(GoldenName::TabStrip, &img);
}

/// A tabbed view: the same strip over a content area, so the selected
/// chip's bottom edge is seen dissolving into the page below it.
#[test]
fn tabbed_view_matches_golden() {
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

    let mut h = Harness::new();
    let img = h.size(UVec2::new(360, 140)).settled_frame(2, scene).image;
    assert_matches_golden(GoldenName::TabbedView, &img);
}

/// Three panes: the divider chrome, one strip per pane, and the dimmed
/// cap that marks the two panes not holding focus.
#[test]
fn dock_split_panes_matches_golden() {
    fn scene(ui: &mut Ui) {
        ui.with_state::<DockFixture, _>(WidgetId::from_hash("visual.dock"), |ui, dock| {
            dock.record(ui);
        });
    }

    let mut h = Harness::new();
    let img = h.size(UVec2::new(520, 220)).settled_frame(2, scene).image;
    assert_matches_golden(GoldenName::DockSplitPanes, &img);
}
