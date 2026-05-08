//! Pin: [`Configure::auto_id`] is `#[track_caller]` and resolves a stable
//! id at the *call site*. Two `.auto_id()` calls on different source lines
//! must produce distinct `WidgetId`s — that's the cross-frame-stability
//! contract for builders that opt into auto ids. Dropping the attribute
//! collapses all calls onto one id (occurrence-counter disambiguation
//! still works within a frame, but state stability degrades).

use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel, text::Text};

#[track_caller]
fn assert_distinct(label: &str, a: WidgetId, b: WidgetId) {
    assert_ne!(
        a, b,
        "{label}: two `.auto_id()` calls on different lines produced the same id — \
         `Configure::auto_id` is missing `#[track_caller]`."
    );
}

fn id_of<W: Configure>(mut w: W) -> WidgetId {
    w.element_mut().id
}

#[test]
fn auto_id_propagates_track_caller_through_every_widget() {
    // For each widget builder, two `.auto_id()` calls on adjacent source
    // lines must yield distinct ids. The case list covers every public
    // constructor so a regression in any one is caught.
    type Case = (&'static str, fn() -> (WidgetId, WidgetId));
    let cases: &[Case] = &[
        ("Button", || {
            (
                id_of(Button::new().auto_id()),
                id_of(Button::new().auto_id()),
            )
        }),
        ("Frame", || {
            (id_of(Frame::new().auto_id()), id_of(Frame::new().auto_id()))
        }),
        ("Grid", || {
            (id_of(Grid::new().auto_id()), id_of(Grid::new().auto_id()))
        }),
        ("Text", || {
            (
                id_of(Text::new("x").auto_id()),
                id_of(Text::new("x").auto_id()),
            )
        }),
        ("Panel::hstack", || {
            (
                id_of(Panel::hstack().auto_id()),
                id_of(Panel::hstack().auto_id()),
            )
        }),
        ("Panel::vstack", || {
            (
                id_of(Panel::vstack().auto_id()),
                id_of(Panel::vstack().auto_id()),
            )
        }),
        ("Panel::zstack", || {
            (
                id_of(Panel::zstack().auto_id()),
                id_of(Panel::zstack().auto_id()),
            )
        }),
        ("Panel::canvas", || {
            (
                id_of(Panel::canvas().auto_id()),
                id_of(Panel::canvas().auto_id()),
            )
        }),
        ("Panel::wrap_hstack", || {
            (
                id_of(Panel::wrap_hstack().auto_id()),
                id_of(Panel::wrap_hstack().auto_id()),
            )
        }),
        ("Panel::wrap_vstack", || {
            (
                id_of(Panel::wrap_vstack().auto_id()),
                id_of(Panel::wrap_vstack().auto_id()),
            )
        }),
    ];
    for (label, mk) in cases {
        let (a, b) = mk();
        assert_distinct(label, a, b);
    }
}

/// Sanity: `id_salt(...)` overrides `auto_id`, so two calls with the
/// same explicit key on different lines produce the *same* id.
#[test]
fn id_salt_overrides_auto_id() {
    assert_eq!(
        id_of(Button::new().id_salt("k")),
        id_of(Button::new().id_salt("k")),
    );
}

/// `Configure::auto_id()` re-derives the id at *its* call site. A helper
/// that builds widgets internally collapses every helper-internal
/// `.auto_id()` to one source location; appending `.auto_id()` at the
/// caller recovers per-line distinctness.
#[test]
fn auto_id_redirects_to_call_site() {
    fn helper() -> Button {
        Button::new().auto_id()
    }
    // Both `helper()` invocations resolve `.auto_id()` inside the helper
    // body — same source line, same id.
    assert_eq!(id_of(helper()), id_of(helper()));
    // With `.auto_id()` on different source lines, the ids diverge.
    let a = id_of(helper().auto_id());
    let b = id_of(helper().auto_id());
    assert_distinct("auto_id() at call site", a, b);
}
