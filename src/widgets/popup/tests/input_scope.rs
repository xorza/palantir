//! What the layers below see while a popup is open, per click-outside mode.

use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use crate::widgets::popup::tests::support::{
    ANCHOR, BODY_H, BODY_W, SURFACE, main_panel_clicked, record_body,
};
use crate::widgets::popup::{ClickOutside, Popup};
use crate::{Sense, Ui};
use glam::Vec2;

/// Pin: pointer gestures over the area outside the popup body must be
/// absorbed by the eater — not leak through to a `Main` widget below
/// that senses the same gesture. Earlier the eater only sensed
/// `CLICK`, so a graph canvas underneath would still receive scroll /
/// pinch / drag while the popup was open.
#[test]
fn outside_pointer_gestures_do_not_leak_to_main() {
    let mut h = UiHarness::new(SURFACE);
    let bg_id = WidgetId::from_hash("scroll-bg");
    let scene = |ui: &mut Ui| {
        // Main-layer background that senses everything pan/zoom-shaped.
        Panel::vstack()
            .id(bg_id)
            .size((Sizing::FILL, Sizing::FILL))
            .sense(Sense::DRAG | Sense::SCROLL | Sense::PINCH)
            .show(ui, |ui| {
                Popup::anchored_to(ANCHOR)
                    .id(WidgetId::from_hash("test-popup"))
                    .click_outside(ClickOutside::Block)
                    .padding(4.0)
                    .show(ui, |ui, _| {
                        Panel::vstack()
                            .id(WidgetId::from_hash("popup-content"))
                            .size((Sizing::fixed(BODY_W), Sizing::fixed(BODY_H)))
                            .show(ui, |_| {});
                    });
            });
    };
    h.frame(scene);

    // Move pointer well outside the popup body, then send a scroll
    // + zoom + middle-drag burst.
    let outside = Vec2::new(300.0, 300.0);
    h.scroll_pixels_at(outside, Vec2::new(0.0, 25.0));
    h.scroll_lines(Vec2::new(0.0, 3.0));
    h.pinch(1.4);
    h.press_button(PointerButton::Middle);
    h.move_to(outside + Vec2::new(40.0, 0.0));
    h.release_button(PointerButton::Middle);

    h.frame(scene);
    let bg = h.ui.response_for(bg_id);
    assert_eq!(
        bg.scroll.pixels,
        Vec2::ZERO,
        "scroll-pixels under popup must not reach Main",
    );
    assert_eq!(
        bg.scroll.lines,
        Vec2::ZERO,
        "scroll-lines under popup must not reach Main",
    );
    assert_eq!(
        bg.scroll.zoom, 1.0,
        "pinch zoom under popup must not reach Main",
    );
    assert!(
        !bg.middle.drag.dragging(),
        "middle-drag under popup must not latch on Main",
    );
}

#[test]
fn click_outside_blocks_main_without_signaling_with_block_mode() {
    let mut h = UiHarness::new(SURFACE);
    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    h.click_at(Vec2::new(300.0, 300.0));

    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    assert!(!dismissed, "`Block` mode must not signal dismissal");
    assert!(
        !main_panel_clicked(&h.ui),
        "`Block` mode must still eat the click — no leak to Main",
    );
}

/// The whole outside-press contract in one table, because the parameter
/// has to *matter*: asserting only `PassThrough`'s half would pass just as
/// well if the eater had been dropped from every mode.
///
/// `signals` is `dismissed` — `Dismiss` is the only mode that reports the
/// press it ate, and `PassThrough` never reports one because it never
/// takes it.
#[test]
fn each_click_outside_mode_decides_whether_main_sees_the_press() {
    for (mode, reaches_main, signals) in [
        (ClickOutside::Block, false, false),
        (ClickOutside::Dismiss, false, true),
        (ClickOutside::PassThrough, true, false),
    ] {
        let mut h = UiHarness::new(SURFACE);
        let mut dismissed = false;
        h.frame(|ui| {
            record_body(ui, mode, &mut dismissed);
        });
        h.click_at(Vec2::new(300.0, 300.0));

        // Both reads OR across passes, for the reason `record_body`
        // already does: a click Main *acts* on makes `Ui::frame` re-run
        // the closure, and pass B sees the edge gone. Reading Main's
        // response after the frame samples that second pass and reports
        // a click that did land as though it never had.
        let mut dismissed = false;
        let mut reached = false;
        h.frame(|ui| {
            record_body(ui, mode, &mut dismissed);
            reached |= main_panel_clicked(ui);
        });
        assert_eq!(
            reached, reaches_main,
            "{mode:?}: whether an outside click reaches Main",
        );
        assert_eq!(dismissed, signals, "{mode:?}: whether it signals dismissal");
    }
}

/// The key-scope claim is the *other* capture, and `PassThrough` drops it
/// too. Worth its own test: a host that could be clicked but not typed
/// into would be just as dead, and the eater cannot be seen from here —
/// `KeyFilter::ALL` silences the layers below whether or not a pointer
/// ever moves.
#[test]
fn only_pass_through_leaves_the_keyboard_to_the_layers_below() {
    use crate::input::shortcut::Shortcut;

    for (mode, main_reads_key) in [
        (ClickOutside::Block, false),
        (ClickOutside::Dismiss, false),
        (ClickOutside::PassThrough, true),
    ] {
        let mut h = UiHarness::new(SURFACE);
        let mut saw = false;
        let mut scene = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("main-bg"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Read from `Main`, under the popup's layer. `F5` rather
                    // than Esc: Esc is the dismiss key the popup itself
                    // consumes, so it could not tell "scope silenced Main"
                    // from "the popup handled it".
                    saw |= ui.key_pressed(Shortcut::key(Key::F5));
                    Popup::anchored_to(ANCHOR)
                        .id(WidgetId::from_hash("test-popup"))
                        .click_outside(mode)
                        .show(ui, |ui, _popup| {
                            Panel::vstack()
                                .id(WidgetId::from_hash("popup-content"))
                                .size((Sizing::fixed(BODY_W), Sizing::fixed(BODY_H)))
                                .show(ui, |_| {});
                        });
                });
        };
        h.frame(&mut scene);
        h.key(Key::F5);
        h.frame(&mut scene);

        assert_eq!(
            saw, main_reads_key,
            "{mode:?}: whether Main still reads the keyboard under the popup",
        );
    }
}

/// A text field inside a popup must be typeable.
///
/// It was not, and the way it failed is worth keeping: `Popup::show`
/// claims the keyboard for its whole body, and `TextEdit` drains the
/// stream that claim gates, so a popup that silenced its own body threw
/// away every keystroke aimed at the field inside it. Nothing in the tree
/// exercised the combination, so it went unnoticed.
///
/// It works because the popup's `KeyFilter::ALL` scope is recorded on
/// `Layer::Popup` — the same layer as its body — and `Scopes::silences`
/// cuts off layers *strictly* below the active one. Same layer, so the
/// body reads on. Both halves are load-bearing: widening that comparison
/// to `>=`, or hoisting the scope onto a layer above the body it wraps,
/// silently breaks typing again, which is what this test is here to
/// catch.
#[test]
fn text_edit_inside_a_popup_receives_typing() {
    use crate::widgets::text_edit::TextEdit;

    let field = WidgetId::from_hash("popup-field");
    let mut buf = String::new();
    let scene = |ui: &mut Ui, buf: &mut String| {
        Popup::anchored_to(glam::Vec2::ZERO)
            .id(WidgetId::from_hash("host"))
            .show(ui, |ui, _handle| {
                TextEdit::new(buf).id(field).show(ui);
            });
    };

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| scene(ui, &mut buf));
    h.request_focus(Some(field));
    h.frame(|ui| scene(ui, &mut buf));

    h.type_text("x");
    h.frame(|ui| scene(ui, &mut buf));

    assert_eq!(
        buf, "x",
        "the popup's keyboard capture must not swallow typing aimed at a \
         field inside its own body",
    );
}

/// Escape resolves to the innermost scope that claims it — so a focused
/// field inside a popup decides, per field, whether one press closes the
/// popup or just blurs the field.
///
/// Both directions are pinned together because the failure mode is a
/// swap: a filter field that keeps `ESCAPE` leaves the popup open around
/// a search box the user can no longer type into, and an inline editor
/// that gives it up loses its cancel *and* tears down the surface behind
/// it. Neither is visible from the widget alone — it takes a popup, a
/// focused field, and one keypress.
#[test]
fn a_field_decides_whether_escape_closes_the_popup_around_it() {
    use crate::input::keyboard::key::Key;
    use crate::widgets::text_edit::TextEdit;

    let field = WidgetId::from_hash("filter-field");

    /// One popup holding one focused field, returning whether the popup
    /// dismissed this frame. `falls_through` picks the archetype.
    fn open(falls_through: bool) -> (bool, Option<WidgetId>) {
        let field = WidgetId::from_hash("filter-field");
        let mut buf = String::new();
        let scene = |ui: &mut Ui, buf: &mut String| {
            let mut dismissed = false;
            Panel::vstack()
                .id(WidgetId::from_hash("main-bg"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let r = Popup::anchored_to(ANCHOR)
                        .id(WidgetId::from_hash("filter-popup"))
                        .click_outside(ClickOutside::Dismiss)
                        .show(ui, |ui, _handle| {
                            let edit = TextEdit::new(buf).id(field);
                            let edit = if falls_through {
                                edit.escape_falls_through()
                            } else {
                                edit
                            };
                            edit.show(ui);
                        });
                    dismissed |= r.dismissed;
                });
            dismissed
        };

        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            scene(ui, &mut buf);
        });
        h.request_focus(Some(field));
        // Two settling frames: the scope path resolves against the
        // previous frame's cascade, so the filter this field declares has
        // to have been recorded once before the press reads it.
        h.frame(|ui| {
            scene(ui, &mut buf);
        });
        h.frame(|ui| {
            scene(ui, &mut buf);
        });
        assert_eq!(h.focused_id(), Some(field), "the field holds focus");

        h.key(Key::Escape);
        let dismissed = h.frame_value(|ui| scene(ui, &mut buf));
        (dismissed, h.focused_id())
    }

    // Default: the field owns Escape. It blurs, and the popup stays open.
    let (dismissed, focused) = open(false);
    assert!(
        !dismissed,
        "an editing field's Esc must not close the popup"
    );
    assert_eq!(focused, None, "…it blurs the field instead");

    // Opted out: Escape walks past the field to the popup's own scope.
    let (dismissed, focused) = open(true);
    assert!(dismissed, "a filter field's Esc closes the popup");
    assert_eq!(
        focused,
        Some(field),
        "…and the field never saw it, so focus is untouched",
    );
}
