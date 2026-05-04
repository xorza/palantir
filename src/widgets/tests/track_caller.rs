//! Regression: every public widget constructor must propagate
//! `#[track_caller]` so two calls on different source lines produce
//! distinct `WidgetId`s. Forgetting the attribute collapses all calls
//! of that constructor onto one auto id — the `Ui::node` occurrence
//! counter still disambiguates within a frame, but cross-frame state
//! stability degrades to global-positional (egui-tier). These tests
//! fail loudly so the regression doesn't ship.

use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel, text::Text};

#[track_caller]
fn assert_distinct(label: &str, a: WidgetId, b: WidgetId) {
    assert_ne!(
        a, b,
        "{label}: two calls on different lines produced the same auto id — \
         the constructor is missing `#[track_caller]` (or one of its callees is)."
    );
}

fn id_of<W: Configure>(mut w: W) -> WidgetId {
    w.element_mut().id
}

#[test]
fn button_new_propagates_track_caller() {
    assert_distinct("Button::new", id_of(Button::new()), id_of(Button::new()));
}

#[test]
fn frame_new_propagates_track_caller() {
    assert_distinct("Frame::new", id_of(Frame::new()), id_of(Frame::new()));
}

#[test]
fn grid_new_propagates_track_caller() {
    assert_distinct("Grid::new", id_of(Grid::new()), id_of(Grid::new()));
}

#[test]
fn text_new_propagates_track_caller() {
    assert_distinct("Text::new", id_of(Text::new("x")), id_of(Text::new("x")));
}

#[test]
fn panel_hstack_propagates_track_caller() {
    assert_distinct(
        "Panel::hstack",
        id_of(Panel::hstack()),
        id_of(Panel::hstack()),
    );
}

#[test]
fn panel_vstack_propagates_track_caller() {
    assert_distinct(
        "Panel::vstack",
        id_of(Panel::vstack()),
        id_of(Panel::vstack()),
    );
}

#[test]
fn panel_zstack_propagates_track_caller() {
    assert_distinct(
        "Panel::zstack",
        id_of(Panel::zstack()),
        id_of(Panel::zstack()),
    );
}

#[test]
fn panel_canvas_propagates_track_caller() {
    assert_distinct(
        "Panel::canvas",
        id_of(Panel::canvas()),
        id_of(Panel::canvas()),
    );
}

#[test]
fn panel_wrap_hstack_propagates_track_caller() {
    assert_distinct(
        "Panel::wrap_hstack",
        id_of(Panel::wrap_hstack()),
        id_of(Panel::wrap_hstack()),
    );
}

#[test]
fn panel_wrap_vstack_propagates_track_caller() {
    assert_distinct(
        "Panel::wrap_vstack",
        id_of(Panel::wrap_vstack()),
        id_of(Panel::wrap_vstack()),
    );
}

/// Sanity: `with_id(...)` overrides the auto id, so two calls with the
/// same explicit key on different lines produce the *same* id — the
/// symmetric counterpart of the tests above. Confirms `auto_id` flips
/// off correctly when an explicit key is supplied.
#[test]
fn with_id_overrides_auto_stability() {
    assert_eq!(
        id_of(Button::new().with_id("k")),
        id_of(Button::new().with_id("k")),
    );
}
