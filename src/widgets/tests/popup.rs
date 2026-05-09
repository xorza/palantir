//! End-to-end tests for `Popup`'s click-outside contract.
//!
//! Both `Block` and `Dismiss` install a full-surface click-eater
//! leaf in the `Popup` layer behind the body. These tests pin:
//! - clicks **inside** the body's rect aren't classified as outside
//!   clicks (no `dismissed`, no eater click);
//! - clicks **outside** the body land on the eater (popup beats Main
//!   in hit-test) and are consumed before reaching Main;
//! - `Dismiss` surfaces the outside-click via `PopupResponse.dismissed`
//!   while `Block` swallows it silently.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::support::testing::{begin, click_at, ui_at};
use crate::tree::element::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::popup::{ClickOutside, Popup, PopupResponse};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 400);
const ANCHOR: Rect = Rect {
    min: Vec2::new(50.0, 50.0),
    size: Size::new(120.0, 80.0),
};

/// Records `Popup` with the given config and returns its
/// `PopupResponse` plus the Main panel's last-frame `clicked` state
/// (so tests can check whether the eater swallowed the click vs.
/// letting it leak to the underlying Main tree).
fn record_with_popup(ui: &mut Ui, config: ClickOutside) -> (PopupResponse, bool) {
    let mut popup_resp: Option<PopupResponse> = None;
    Panel::vstack()
        .id_salt("main-bg")
        .size((Sizing::FILL, Sizing::FILL))
        .sense(crate::Sense::CLICK)
        .show(ui, |ui| {
            let r = Popup::anchored_to(ANCHOR)
                .id_salt("test-popup")
                .click_outside(config)
                .padding(4.0)
                .show(ui, |ui| {
                    Panel::vstack()
                        .id_salt("popup-content")
                        .size((Sizing::Fixed(100.0), Sizing::Fixed(60.0)))
                        .show(ui, |_| {});
                });
            popup_resp = Some(r);
        });
    // Read the Main panel's response post-recording.
    let main_id = crate::tree::widget_id::WidgetId::from_hash("main-bg");
    let main_panel_clicked = ui.response_for(main_id).clicked;
    (popup_resp.unwrap(), main_panel_clicked)
}

/// A press *inside* the popup body's rect must not be reported as an
/// outside click. With `Dismiss`, the host's open flag stays put.
#[test]
fn click_inside_popup_does_not_dismiss() {
    let mut ui = ui_at(SURFACE);
    let (_, _) = record_with_popup(&mut ui, ClickOutside::Dismiss);
    ui.end_frame();

    // Click at the center of the popup's anchor rect — well inside
    // the body's arranged rect.
    let inside = Vec2::new(
        ANCHOR.min.x + ANCHOR.size.w * 0.5,
        ANCHOR.min.y + ANCHOR.size.h * 0.5,
    );
    click_at(&mut ui, inside);

    begin(&mut ui, SURFACE);
    let (resp, main_clicked) = record_with_popup(&mut ui, ClickOutside::Dismiss);
    ui.end_frame();
    assert!(
        !resp.dismissed,
        "click inside body must not signal dismissal",
    );
    assert!(!main_clicked, "click inside body must not leak to Main",);
}

/// A press *outside* the popup body but inside the surface lands on
/// the click-eater. With `Dismiss`, `dismissed` is set; Main's
/// background panel never sees the click (eater beats Main in
/// hit-test priority).
#[test]
fn click_outside_popup_dismisses_and_blocks_main() {
    let mut ui = ui_at(SURFACE);
    record_with_popup(&mut ui, ClickOutside::Dismiss);
    ui.end_frame();

    // (300, 300) is on the surface but well outside the popup
    // anchor `[50..170] × [50..130]`. Falls through to the eater.
    click_at(&mut ui, Vec2::new(300.0, 300.0));

    begin(&mut ui, SURFACE);
    let (resp, main_clicked) = record_with_popup(&mut ui, ClickOutside::Dismiss);
    ui.end_frame();
    assert!(
        resp.dismissed,
        "outside click with `Dismiss` must signal dismissal",
    );
    assert!(
        !main_clicked,
        "outside click must be eaten by the popup eater, not leak to Main",
    );
}

/// `Ui::run_frame` settles popup dismissal in a single host call.
/// Pass 1 records the open popup, sees the eater click, sets
/// `dismissed = true`, host flips `open = false`. Pass 2 sees
/// `open == false` and records no popup. The painted tree (pass 2)
/// has no popup-layer widgets — no stale frame ever reaches submit.
#[test]
fn run_frame_settles_popup_dismissal_in_one_call() {
    use crate::layout::types::display::Display;

    let mut ui = ui_at(SURFACE);
    // Frame 0: popup open, no input. Single pass.
    let mut open = true;
    {
        let open = &mut open;
        Panel::vstack()
            .id_salt("main-bg")
            .size((Sizing::FILL, Sizing::FILL))
            .show(&mut ui, |ui| {
                if *open {
                    let r = Popup::anchored_to(ANCHOR)
                        .id_salt("test-popup")
                        .click_outside(ClickOutside::Dismiss)
                        .show(ui, |ui| {
                            Panel::vstack()
                                .id_salt("popup-content")
                                .size((Sizing::Fixed(100.0), Sizing::Fixed(60.0)))
                                .show(ui, |_| {});
                        });
                    if r.dismissed {
                        *open = false;
                    }
                }
            });
        ui.end_frame();
    }

    // Pop the press outside the popup body.
    click_at(&mut ui, Vec2::new(300.0, 300.0));

    // Frame 1: run_frame should re-record once dismissal fires, so
    // pass 2's painted tree has zero `Layer::Popup` nodes.
    let display = Display::from_physical(SURFACE, 1.0);
    let _frame_out = ui.run_frame(display, std::time::Duration::ZERO, |ui| {
        Panel::vstack()
            .id_salt("main-bg")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                if open {
                    let r = Popup::anchored_to(ANCHOR)
                        .id_salt("test-popup")
                        .click_outside(ClickOutside::Dismiss)
                        .show(ui, |ui| {
                            Panel::vstack()
                                .id_salt("popup-content")
                                .size((Sizing::Fixed(100.0), Sizing::Fixed(60.0)))
                                .show(ui, |_| {});
                        });
                    if r.dismissed {
                        open = false;
                    }
                }
            });
    });
    assert!(!open, "host flag must flip to false in pass 1");
    assert_eq!(
        ui.forest.tree(crate::tree::Layer::Popup).records.len(),
        0,
        "painted tree (pass 2) must contain no Popup-layer widgets",
    );
}

/// `Block` mode swallows outside clicks silently — no dismissal
/// signal, but Main still doesn't see the click.
#[test]
fn click_outside_blocks_main_without_signaling_with_block_mode() {
    let mut ui = ui_at(SURFACE);
    record_with_popup(&mut ui, ClickOutside::Block);
    ui.end_frame();

    click_at(&mut ui, Vec2::new(300.0, 300.0));

    begin(&mut ui, SURFACE);
    let (resp, main_clicked) = record_with_popup(&mut ui, ClickOutside::Block);
    ui.end_frame();
    assert!(!resp.dismissed, "`Block` mode must not signal dismissal",);
    assert!(
        !main_clicked,
        "`Block` mode must still eat the click — no leak to Main",
    );
}
