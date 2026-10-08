//! Tab and dock fixtures: the chip row's chrome, and the pane tree the dock walks it onto.

use glam::UVec2;
use palantir::internals::frame_fixture::dock_fixture::DockFixture;
use palantir::{
    Configure, Panel, Sizing, TabBadge, TabItem, TabStrip, TabbedView, Text, Ui, WidgetId,
};

use crate::golden_name::GoldenName;
use crate::goldens::assert_settled_scene_matches_golden;

/// A strip alone: a selected accent-capped chip, an inked badge, close buttons.
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

    assert_settled_scene_matches_golden(GoldenName::TabStrip, UVec2::new(360, 76), 2, scene);
}

/// A tabbed view: the selected chip's bottom edge dissolves into the page below.
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

    assert_settled_scene_matches_golden(GoldenName::TabbedView, UVec2::new(360, 140), 2, scene);
}

/// Three panes: divider chrome, a strip per pane, and the dimmed cap on the two unfocused panes.
#[test]
fn dock_split_panes_matches_golden() {
    fn scene(ui: &mut Ui) {
        ui.with_state::<DockFixture, _>(WidgetId::from_hash("visual.dock"), |ui, dock| {
            dock.record(ui);
        });
    }

    assert_settled_scene_matches_golden(GoldenName::DockSplitPanes, UVec2::new(520, 220), 2, scene);
}
