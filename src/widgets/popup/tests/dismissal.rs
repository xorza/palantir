//! What closes a popup, and how long it takes to settle.

use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use crate::widgets::popup::tests::support::{
    ANCHOR, BODY_H, BODY_W, SURFACE, main_panel_clicked, record_body,
};
use crate::widgets::popup::{ClickOutside, Popup};
use crate::{Sense, Ui};
use glam::Vec2;

#[test]
fn click_inside_popup_does_not_dismiss() {
    let mut h = UiHarness::new(SURFACE);
    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    let inside = Vec2::new(ANCHOR.x + BODY_W * 0.5, ANCHOR.y + BODY_H * 0.5);
    h.click_at(inside);

    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    assert!(!dismissed, "click inside body must not signal dismissal");
    assert!(
        !main_panel_clicked(&h.ui),
        "click inside body must not leak to Main"
    );
}

/// Every pointer button dismisses, not just the primary. The secondary
/// case is the one users hit: a context menu opens on right-click, so
/// right-clicking elsewhere is the natural way to move or drop it — and
/// while only `left` was read, that press was absorbed by the eater and
/// then ignored, leaving the menu stuck open.
#[test]
fn outside_click_dismisses_on_any_button_and_blocks_main() {
    for button in PointerButton::all() {
        let mut h = UiHarness::new(SURFACE);
        let mut dismissed = false;
        h.frame(|ui| {
            record_body(ui, ClickOutside::Dismiss, &mut dismissed);
        });
        h.click_button_at(button, Vec2::new(300.0, 300.0));

        let mut dismissed = false;
        h.frame(|ui| {
            record_body(ui, ClickOutside::Dismiss, &mut dismissed);
        });
        assert!(
            dismissed,
            "{button:?} outside click with `Dismiss` must signal dismissal",
        );
        assert!(
            !main_panel_clicked(&h.ui),
            "{button:?} outside click must be eaten by the popup eater, not leak to Main",
        );
    }
}

#[test]
fn escape_dismisses_dismiss_popup_but_not_block() {
    // `Dismiss`: Esc folds into `dismissed`.
    let mut h = UiHarness::new(SURFACE);
    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    h.key(Key::Escape);
    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    assert!(dismissed, "Esc dismisses a `Dismiss` popup");

    // `Block`: Esc is ignored (stop-the-world prompts close only on the
    // host's terms).
    let mut h = UiHarness::new(SURFACE);
    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    h.key(Key::Escape);
    let mut dismissed = false;
    h.frame(|ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    assert!(!dismissed, "Esc does not dismiss a `Block` popup");
}

/// `Ui::frame` settles popup dismissal in a single host call.
/// Pass 1 records the open popup, sees the eater click, sets
/// `dismissed = true`, host flips `open = false`. Pass 2 sees
/// `open == false` and records no popup. The painted tree (pass 2)
/// has no popup-layer widgets — no stale frame ever reaches submit.
#[test]
fn run_frame_settles_popup_dismissal_in_one_call() {
    let mut h = UiHarness::new(SURFACE);
    let mut open = true;
    let scene = |ui: &mut Ui, open: &mut bool| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                if *open {
                    let r = Popup::anchored_to(ANCHOR)
                        .id(WidgetId::from_hash("test-popup"))
                        .click_outside(ClickOutside::Dismiss)
                        .show(ui, |ui, _popup| {
                            Panel::vstack()
                                .id(WidgetId::from_hash("popup-content"))
                                .size((Sizing::fixed(100.0), Sizing::fixed(60.0)))
                                .show(ui, |_| {});
                        });
                    if r.dismissed {
                        *open = false;
                    }
                }
            });
    };
    h.frame(|ui| scene(ui, &mut open));
    h.click_at(Vec2::new(300.0, 300.0));
    h.frame(|ui| scene(ui, &mut open));
    assert!(!open, "host flag must flip to false in pass 1");
    assert_eq!(
        h.ui.tree(Layer::Popup).records.len(),
        0,
        "painted tree (pass 2) must contain no Popup-layer widgets",
    );
}

/// A dismissed popup hands input back on the very next frame.
///
/// The case `PopupHandle`'s close has always been *for* and, until the
/// frame stamp on `Scopes::closed`, never actually did: a dismissal is
/// action input, so its frame records twice, and pass B used to wipe
/// pass A's close without being able to re-issue it — the dismissing
/// edge is drained between the passes. `Main` then stayed cut off for a
/// frame, long enough to swallow the keystroke or scroll that lands
/// where the popup used to be.
#[test]
fn a_dismissed_popup_stops_owning_input_the_next_frame() {
    use crate::scene::layer::Layer;

    let content = WidgetId::from_hash("popup-content");
    let mut h = UiHarness::new(SURFACE);
    let mut dismissed = false;
    let build = |ui: &mut Ui, open: bool, dismissed: &mut bool| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .sense(Sense::CLICK)
            .show(ui, |ui| {
                if !open {
                    return;
                }
                let r = Popup::anchored_to(ANCHOR)
                    .id(WidgetId::from_hash("test-popup"))
                    .click_outside(ClickOutside::Dismiss)
                    .show(ui, |ui, _popup| {
                        Panel::vstack()
                            .id(content)
                            .size((Sizing::fixed(BODY_W), Sizing::fixed(BODY_H)))
                            .show(ui, |_| {});
                    });
                *dismissed |= r.dismissed;
            });
    };

    h.frame(|ui| build(ui, true, &mut dismissed));
    h.frame(|ui| build(ui, true, &mut dismissed));

    // Escape dismisses it. Focus makes the wake-gate deliver the chord.
    h.ui.input_mut().focused = Some(content);
    h.key(Key::Escape);
    h.frame(|ui| build(ui, true, &mut dismissed));
    assert!(
        dismissed,
        "escape must dismiss a ClickOutside::Dismiss popup"
    );

    // Host stops showing it. `Main` must read again immediately — the
    // popup is still in last frame's cascade, so only the close makes
    // this true. Counted inside the record, the only place the queue is
    // live, and maxed across the double-layout passes.
    h.ui.input_mut().focused = Some(WidgetId::from_hash("main-bg"));
    h.key(Key::Escape);
    let mut seen = 0usize;
    h.frame(|ui| {
        build(ui, false, &mut dismissed);
        seen = seen.max(ui.input().keyboard_events(Layer::Main).len());
    });
    assert_eq!(seen, 1, "the frame after dismissal must reach Main");
}
