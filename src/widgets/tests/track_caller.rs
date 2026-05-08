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
fn constructors_propagate_track_caller() {
    // Each pair calls the same constructor twice on adjacent source lines.
    // `#[track_caller]` makes the two calls produce distinct auto ids;
    // dropping the attribute on any constructor (or one of its callees)
    // collapses the pair to identical ids.
    type Case = (&'static str, fn() -> (WidgetId, WidgetId));
    let cases: &[Case] = &[
        ("Button::new", || {
            (id_of(Button::new()), id_of(Button::new()))
        }),
        ("Frame::new", || (id_of(Frame::new()), id_of(Frame::new()))),
        ("Grid::new", || (id_of(Grid::new()), id_of(Grid::new()))),
        ("Text::new", || {
            (id_of(Text::new("x")), id_of(Text::new("x")))
        }),
        ("Panel::hstack", || {
            (id_of(Panel::hstack()), id_of(Panel::hstack()))
        }),
        ("Panel::vstack", || {
            (id_of(Panel::vstack()), id_of(Panel::vstack()))
        }),
        ("Panel::zstack", || {
            (id_of(Panel::zstack()), id_of(Panel::zstack()))
        }),
        ("Panel::canvas", || {
            (id_of(Panel::canvas()), id_of(Panel::canvas()))
        }),
        ("Panel::wrap_hstack", || {
            (id_of(Panel::wrap_hstack()), id_of(Panel::wrap_hstack()))
        }),
        ("Panel::wrap_vstack", || {
            (id_of(Panel::wrap_vstack()), id_of(Panel::wrap_vstack()))
        }),
    ];
    for (label, mk) in cases {
        let (a, b) = mk();
        assert_distinct(label, a, b);
    }
}

/// Sanity: `id_salt(...)` overrides the auto id, so two calls with the
/// same explicit key on different lines produce the *same* id — the
/// symmetric counterpart of the tests above. Confirms `auto_id` flips
/// off correctly when an explicit key is supplied.
#[test]
fn id_salt_overrides_auto_stability() {
    assert_eq!(
        id_of(Button::new().id_salt("k")),
        id_of(Button::new().id_salt("k")),
    );
}
