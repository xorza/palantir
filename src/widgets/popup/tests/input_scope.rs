//! What the layers below see while a popup is open, per click-outside mode.

use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::popup::click_outside::ClickOutside;
use crate::widgets::popup::tests::support::{ANCHOR, BODY_H, BODY_W, SURFACE, frame_body};
use crate::{Sense, Ui};
use glam::Vec2;

/// Pointer gestures outside the popup body must be absorbed by the eater, not
/// leak to a `Main` widget. `PassThrough` is the control: the burst reaches `Main` there.
#[test]
fn outside_pointer_gestures_do_not_leak_to_main() {
    for (mode, leaks) in [
        (ClickOutside::Block, false),
        (ClickOutside::PassThrough, true),
    ] {
        let mut h = UiHarness::new(SURFACE);
        let bg_id = WidgetId::from_hash("scroll-bg");
        let scene = |ui: &mut Ui| {
            Panel::vstack()
                .id(bg_id)
                .size((Sizing::FILL, Sizing::FILL))
                .sense(Sense::DRAG | Sense::SCROLL | Sense::PINCH)
                .show(ui, |ui| {
                    Popup::new(Anchor::at_point(ANCHOR))
                        .id(WidgetId::from_hash("test-popup"))
                        .click_outside(mode)
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

        let outside = Vec2::new(300.0, 300.0);
        h.scroll_pixels_at(outside, Vec2::new(0.0, 25.0));
        h.scroll_lines(Vec2::new(0.0, 3.0));
        h.pinch(1.4);
        h.press_button(PointerButton::Middle);
        h.move_to(outside + Vec2::new(40.0, 0.0));

        let bg = h.response_in(bg_id, scene);
        assert_eq!(
            bg.scroll.pixels != Vec2::ZERO,
            leaks,
            "{mode:?}: scroll pixels"
        );
        assert_eq!(
            bg.scroll.lines != Vec2::ZERO,
            leaks,
            "{mode:?}: scroll lines"
        );
        assert_eq!(bg.scroll.zoom.get() != 1.0, leaks, "{mode:?}: pinch zoom");
        assert_eq!(bg.middle.drag.is_live(), leaks, "{mode:?}: middle drag");
        h.release_button(PointerButton::Middle);
    }
}

#[test]
fn click_outside_blocks_main_without_signaling_with_block_mode() {
    let mut h = UiHarness::new(SURFACE);
    frame_body(&mut h, ClickOutside::Block);
    h.click_at(Vec2::new(300.0, 300.0));

    let pass = frame_body(&mut h, ClickOutside::Block);
    assert!(!pass.dismissed, "`Block` mode must not signal dismissal");
    assert!(
        !pass.main_clicked,
        "`Block` mode must still eat the click — no leak to Main",
    );
}

/// The whole outside-press contract in one table; `signals` is `dismissed`, which only `Dismiss` reports.
#[test]
fn each_click_outside_mode_decides_whether_main_sees_the_press() {
    for (mode, reaches_main, signals) in [
        (ClickOutside::Block, false, false),
        (ClickOutside::Dismiss, false, true),
        (ClickOutside::PassThrough, true, false),
    ] {
        let mut h = UiHarness::new(SURFACE);
        frame_body(&mut h, mode);
        h.click_at(Vec2::new(300.0, 300.0));

        let pass = frame_body(&mut h, mode);
        assert_eq!(
            pass.main_clicked, reaches_main,
            "{mode:?}: whether an outside click reaches Main",
        );
        assert_eq!(
            pass.dismissed, signals,
            "{mode:?}: whether it signals dismissal"
        );
    }
}

/// The key-scope claim is the other capture, and `PassThrough` drops it too:
/// a host that can be clicked but not typed into is just as dead.
#[test]
fn only_pass_through_leaves_the_keyboard_to_the_layers_below() {
    use crate::input::shortcut::Shortcut;

    for (mode, main_reads_key) in [
        (ClickOutside::Block, false),
        (ClickOutside::Dismiss, false),
        (ClickOutside::PassThrough, true),
    ] {
        let mut h = UiHarness::new(SURFACE);
        let scene = |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("main-bg"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Read from `Main`; `F5`, not Esc, which the popup itself consumes.
                    let saw = ui.key_pressed(Shortcut::key(Key::F5));
                    Popup::new(Anchor::at_point(ANCHOR))
                        .id(WidgetId::from_hash("test-popup"))
                        .click_outside(mode)
                        .show(ui, |ui, _popup| {
                            Panel::vstack()
                                .id(WidgetId::from_hash("popup-content"))
                                .size((Sizing::fixed(BODY_W), Sizing::fixed(BODY_H)))
                                .show(ui, |_| {});
                        });
                    saw
                })
                .inner
        };
        h.frame(|ui| {
            scene(ui);
        });
        h.key(Key::F5);
        let saw = h.frame_value(scene);

        assert_eq!(
            saw, main_reads_key,
            "{mode:?}: whether Main still reads the keyboard under the popup",
        );
    }
}

/// A text field inside a popup must be typeable. The `KeyFilter::ALL` scope is
/// recorded on `Layer::Popup`, the body's layer, and `Scopes::silences` cuts
/// off only strictly lower layers; `>=`, or hoisting the scope above the body,
/// breaks typing.
#[test]
fn text_edit_inside_a_popup_receives_typing() {
    use crate::widgets::text_edit::TextEdit;

    let field = WidgetId::from_hash("popup-field");
    let mut buf = String::new();
    let scene = |ui: &mut Ui, buf: &mut String| {
        Popup::new(Anchor::at_point(Vec2::ZERO))
            .id(WidgetId::from_hash("host"))
            .show(ui, |ui, _handle| {
                TextEdit::new(buf).id(field).show(ui);
            });
    };

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| scene(ui, &mut buf));
    h.set_focus(field);
    h.frame(|ui| scene(ui, &mut buf));

    h.type_text("x");
    h.frame(|ui| scene(ui, &mut buf));

    assert_eq!(
        buf, "x",
        "the popup's keyboard capture must not swallow typing aimed at a \
         field inside its own body",
    );
}

/// Escape resolves to the innermost scope that claims it, so a focused field
/// in a popup decides whether one press closes the popup or only blurs the
/// field. Both directions are pinned since the failure is a swap.
#[test]
fn a_field_decides_whether_escape_closes_the_popup_around_it() {
    /// One popup with one focused field; returns whether it dismissed. `falls_through` picks the archetype.
    fn open(falls_through: bool) -> (bool, Option<WidgetId>) {
        let field = WidgetId::from_hash("filter-field");
        let mut buf = String::new();
        let scene = |ui: &mut Ui, buf: &mut String| {
            Panel::vstack()
                .id(WidgetId::from_hash("main-bg"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Popup::new(Anchor::at_point(ANCHOR))
                        .id(WidgetId::from_hash("filter-popup"))
                        .click_outside(ClickOutside::Dismiss)
                        .show(ui, |ui, _handle| {
                            TextEdit::new(buf)
                                .id(field)
                                .escape_falls_through(falls_through)
                                .show(ui);
                        })
                        .dismissed
                })
                .inner
        };

        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            scene(ui, &mut buf);
        });
        h.set_focus(field);
        // Two settling frames: the scope resolves from the previous cascade.
        h.frame(|ui| {
            scene(ui, &mut buf);
        });
        h.frame(|ui| {
            scene(ui, &mut buf);
        });
        assert_eq!(h.focus(), Some(field), "the field holds focus");

        h.key(Key::Escape);
        let dismissed = h.frame_value(|ui| scene(ui, &mut buf));
        (dismissed, h.focus())
    }

    use crate::input::keyboard::key::Key;
    use crate::widgets::text_edit::TextEdit;

    let field = WidgetId::from_hash("filter-field");

    let (dismissed, focused) = open(false);
    assert!(
        !dismissed,
        "an editing field's Esc must not close the popup"
    );
    assert_eq!(focused, None, "…it blurs the field instead");

    let (dismissed, focused) = open(true);
    assert!(dismissed, "a filter field's Esc closes the popup");
    assert_eq!(
        focused,
        Some(field),
        "…and the field never saw it, so focus is untouched",
    );
}
