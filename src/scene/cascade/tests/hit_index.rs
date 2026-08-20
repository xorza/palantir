//! Which rows reach the hit index, in what order, carrying which rect.

use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

#[test]
fn hits_track_only_sensing_or_focusable_rows_in_paint_order() {
    use crate::input::sense::Sense;
    use crate::widgets::frame::Frame;

    let inert = WidgetId::from_hash("inert");
    let hover = WidgetId::from_hash("hover");
    let focus = WidgetId::from_hash("focus");
    let disabled = WidgetId::from_hash("disabled");
    let popup_scroll = WidgetId::from_hash("popup-scroll");
    let mut h = UiHarness::new(UVec2::splat(100));
    h.frame(|ui| {
        Panel::zstack()
            .auto_id()
            .size(Sizing::fixed(100.0))
            .show(ui, |ui| {
                Frame::new().id(inert).size(Sizing::FILL).show(ui);
                Frame::new()
                    .id(hover)
                    .size(Sizing::FILL)
                    .sense(Sense::HOVER)
                    .show(ui);
                Frame::new()
                    .id(focus)
                    .size(Sizing::FILL)
                    .focusable(true)
                    .show(ui);
                Frame::new()
                    .id(disabled)
                    .size(Sizing::FILL)
                    .sense(Sense::CLICK)
                    .focusable(true)
                    .disabled(true)
                    .show(ui);
            });
        ui.layer(Layer::Popup).show(|ui| {
            Frame::new()
                .id(popup_scroll)
                .size(Sizing::FILL)
                .sense(Sense::SCROLL)
                .show(ui);
        });
    });

    // `hits` is interactive-rows-only, in paint order, and carries its
    // own geometry — so identity is all that needs asserting here.
    assert_eq!(
        h.ui.cascade()
            .hits
            .iter()
            .map(|r| r.widget_id)
            .collect::<Vec<_>>(),
        [hover, focus, popup_scroll],
    );
    let pos = Vec2::splat(50.0);
    assert_eq!(h.ui.cascade().hit_test(pos, Sense::hovers), Some(hover),);
    assert_eq!(h.ui.cascade().hit_test(pos, Sense::clicks), None);
    // One walk must agree with the two separate filters above it: the
    // press path resolves both from a single scan.
    let press = h.ui.cascade().hit_test_press(pos);
    assert_eq!(press.focus, Some(focus));
    assert_eq!(press.click, None);
    let targets = h.ui.cascade().hit_test_targets(pos);
    assert_eq!(targets.hover, Some(hover));
    assert_eq!(targets.scroll, Some(popup_scroll));
    assert_eq!(targets.pinch, None);

    h.frame(|ui| {
        Frame::new().id(inert).size(Sizing::FILL).show(ui);
    });
    assert_eq!(h.ui.cascade().hits.len(), 0);
    assert_eq!(
        h.ui.response_for(inert).layout_rect,
        Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
        "inert widgets remain addressable through the all-widget by-id snapshot",
    );
}

/// The interactive table is self-sufficient: every `HitRow` carries the
/// same `rect` the node's `EntryRow` does, so the hit scan never needs
/// to reach back into `entries`. If those two ever disagreed, hit
/// testing would silently use stale geometry.
#[test]
fn hit_rows_carry_the_entry_rect() {
    let mut h = UiHarness::new(UVec2::splat(300));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size(Sizing::fixed(200.0))
            .sense(crate::input::sense::Sense::HOVER)
            .show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("inner"))
                    .size(Sizing::fixed(60.0))
                    .sense(crate::input::sense::Sense::CLICK)
                    .show(ui, |_| {});
            });
    });

    let cascade = h.ui.cascade();
    assert!(!cascade.hits.is_empty(), "expected interactive rows");
    for row in &cascade.hits {
        let entry_idx = cascade
            .locate(row.widget_id)
            .expect("hit row's widget must be locatable")
            .entry_idx as usize;
        assert_eq!(
            row.rect, cascade.entries[entry_idx].rect,
            "HitRow::rect must equal the node's EntryRow::rect for {:?}",
            row.widget_id,
        );
    }
}
