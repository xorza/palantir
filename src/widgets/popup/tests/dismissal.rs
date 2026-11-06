//! What closes a popup, and how long it takes to settle.

use crate::host::harness::Harness;
use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::click_outside::ClickOutside;
use crate::widget_core::configure::Configure;
use crate::widget_core::overlay_response::OverlayClose;
use crate::widgets::menu::menu_item::MenuItem;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::popup::tests::support::{ANCHOR, BODY_H, BODY_W, SURFACE, frame_body};
use glam::Vec2;

#[test]
fn click_inside_popup_does_not_dismiss() {
    let mut h = Harness::mono(SURFACE);
    frame_body(&mut h, ClickOutside::Dismiss);
    let inside = Vec2::new(ANCHOR.x + BODY_W * 0.5, ANCHOR.y + BODY_H * 0.5);
    h.click(inside);

    let pass = frame_body(&mut h, ClickOutside::Dismiss);
    assert!(
        !pass.dismissed,
        "click inside body must not signal dismissal"
    );
    assert!(
        !pass.main_clicked,
        "click inside body must not leak to Main"
    );
}

/// Every pointer button dismisses, not just the primary: a right-click elsewhere must drop a context menu rather than be absorbed by the barrier.
#[test]
fn outside_click_dismisses_on_any_button_and_blocks_main() {
    for button in PointerButton::ALL {
        let mut h = Harness::mono(SURFACE);
        frame_body(&mut h, ClickOutside::Dismiss);
        h.press(Vec2::new(300.0, 300.0), button);
        h.release(button);

        let pass = frame_body(&mut h, ClickOutside::Dismiss);
        assert!(
            pass.dismissed,
            "{button:?} outside click with `Dismiss` must signal dismissal",
        );
        assert!(
            !pass.main_clicked,
            "{button:?} outside click must be eaten by the popup barrier, not leak to Main",
        );
    }
}

#[test]
fn escape_dismisses_dismiss_popup_but_not_block() {
    // `Dismiss`: Esc folds into `dismissed`.
    let mut h = Harness::mono(SURFACE);
    frame_body(&mut h, ClickOutside::Dismiss);
    h.key(Key::Escape);
    assert!(
        frame_body(&mut h, ClickOutside::Dismiss).dismissed,
        "Esc dismisses a `Dismiss` popup",
    );

    // `Block`: Esc is ignored.
    let mut h = Harness::mono(SURFACE);
    frame_body(&mut h, ClickOutside::Block);
    h.key(Key::Escape);
    assert!(
        !frame_body(&mut h, ClickOutside::Block).dismissed,
        "Esc does not dismiss a `Block` popup",
    );
}

/// `FrameLoop::frame` settles dismissal in one host call: pass 1 sees the barrier click and sets `dismissed`, the host flips `open`, pass 2 records no popup, so no stale frame reaches submit.
#[test]
fn run_frame_settles_popup_dismissal_in_one_call() {
    let mut h = Harness::mono(SURFACE);
    let mut open = true;
    let scene = |ui: &mut Ui, open: &mut bool| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                if *open {
                    let r = Popup::new()
                        .anchor(Anchor::at_point(ANCHOR))
                        .id(WidgetId::from_hash("test-popup"))
                        .click_outside(ClickOutside::Dismiss)
                        .show(ui, |ui| {
                            Panel::vstack()
                                .id(WidgetId::from_hash("popup-content"))
                                .size((Sizing::fixed(100.0), Sizing::fixed(60.0)))
                                .show(ui, |_| {});
                        });
                    if r.close == Some(OverlayClose::Dismissed) {
                        *open = false;
                    }
                }
            });
    };
    h.frame(|ui| scene(ui, &mut open));
    h.click(Vec2::new(300.0, 300.0));
    h.frame(|ui| scene(ui, &mut open));
    assert!(!open, "host flag must flip to false in pass 1");
    assert_eq!(
        h.ui().tree(Layer::Popup).records.len(),
        0,
        "painted tree (pass 2) must contain no Popup-layer widgets",
    );
}

/// A dismissed popup hands input back the very next frame. A dismissal is action input, so its frame records twice; pass B must not wipe pass A's close (the edge is drained between passes), or `Main` stays cut off and swallows the next keystroke or scroll.
#[test]
fn a_dismissed_popup_stops_owning_input_the_next_frame() {
    use crate::scene::layer::Layer;

    let content = WidgetId::from_hash("popup-content");
    let mut h = Harness::mono(SURFACE);
    let build = |ui: &mut Ui, open: bool| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .sense(Sense::CLICK)
            .show(ui, |ui| {
                open && Popup::new()
                    .anchor(Anchor::at_point(ANCHOR))
                    .id(WidgetId::from_hash("test-popup"))
                    .click_outside(ClickOutside::Dismiss)
                    .show(ui, |ui| {
                        Panel::vstack()
                            .id(content)
                            .size((Sizing::fixed(BODY_W), Sizing::fixed(BODY_H)))
                            .show(ui, |_| {});
                    })
                    .close
                    == Some(OverlayClose::Dismissed)
            })
            .inner
    };

    h.prime(2, |ui| {
        build(ui, true);
    });

    // Escape dismisses it; focus makes the wake-gate deliver the chord.
    h.ui_mut().input_mut().set_focus(Some(content));
    h.key(Key::Escape);
    assert!(
        h.frame_value(|ui| build(ui, true)),
        "escape must dismiss a ClickOutside::Dismiss popup"
    );

    // Host stops showing it. `Main` must read again at once, though the popup is still in last frame's cascade; counted inside the record, the only place the queue is live.
    h.ui_mut()
        .input_mut()
        .set_focus(Some(WidgetId::from_hash("main-bg")));
    h.key(Key::Escape);
    let seen = h.frame_value(|ui| {
        build(ui, false);
        ui.input().keyboard_events(Layer::Main.into()).len()
    });
    assert_eq!(seen, 1, "the frame after dismissal must reach Main");
}

/// A row closes the overlay it sits in through the scope, with no handle: a clicked row in a bare
/// popup inside another popup closes the inner popup alone.
#[test]
fn a_clicked_row_closes_the_overlay_it_sits_in() {
    let row = WidgetId::from_hash("row");
    let mut record = |ui: &mut Ui| {
        let outer = Popup::new()
            .anchor(Anchor::at_point(Vec2::new(10.0, 10.0)))
            .id(WidgetId::from_hash("outer"))
            .show(ui, |ui| {
                Popup::new()
                    .anchor(Anchor::at_point(Vec2::new(200.0, 200.0)))
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        MenuItem::new("Row").id(row).show(ui);
                    })
                    .close
                    == Some(OverlayClose::Requested)
            });
        [outer.close == Some(OverlayClose::Requested), outer.inner]
    };
    let mut h = Harness::mono(SURFACE);
    assert_eq!(h.frame_value(&mut record), [false, false]);
    let at = h.center_of(row);
    h.click(at);
    assert_eq!(h.frame_value(&mut record), [false, true]);
}
