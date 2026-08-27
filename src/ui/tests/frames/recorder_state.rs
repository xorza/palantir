//! Theme sharing, and a subtree disabled between frames.

use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::widgets::{button::Button, panel::Panel};
use glam::Vec2;

/// The interaction half of `response_for` routes against the one-frame
/// -stale cascade, so on the frame a subtree becomes disabled a widget
/// could otherwise observe `hovered`/`clicked` alongside
/// `disabled == true` — a combination the steady-state hit index never
/// produces (disabled entries carry `Sense::NONE`), and one that lets
/// a click land on just-disabled UI.
#[test]
fn freshly_disabled_subtree_masks_stale_interactions() {
    let target = WidgetId::from_hash("target");
    let mut h = UiHarness::new(SURFACE);
    let run = |h: &mut UiHarness, disabled: bool| {
        let mut resp = None;
        h.frame(|ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("wrap"))
                .disabled(disabled)
                .show(ui, |ui| {
                    resp = Some(ui.response_for(target));
                    Button::new().label("hi").id(target).show(ui);
                });
        });
        resp.unwrap()
    };
    run(&mut h, false);
    h.move_to(Vec2::new(10.0, 10.0));
    let enabled = run(&mut h, false);
    assert!(enabled.hovered, "sanity: pointer hovers the button");
    assert!(!enabled.disabled);
    // Disable frame: stale cascade still routes the hover; the read
    // must mask it.
    let disabled = run(&mut h, true);
    assert!(disabled.disabled, "ancestor-disabled ORs in lag-free");
    assert!(
        !disabled.hovered,
        "interactions must mask on the disable frame"
    );

    use crate::primitives::color::ColorF16;
    use crate::scene::shapes::paint::ShapeBrush;

    let self_id = WidgetId::from_hash("self-disabled");
    let disabled_fill = Color::rgb(0.8, 0.1, 0.2);
    let mut style = h.ui.theme.button.clone();
    style.looks.disabled.background = Background::fill(disabled_fill);
    let response = h.frame_value(|ui| {
        Button::new()
            .id(self_id)
            .label("disabled")
            .style(&style)
            .disabled(true)
            .show(ui)
            .snapshot()
    });
    assert!(
        response.disabled,
        "a self-disabled widget reports disabled on its own first frame, \
         before the cascade has seen it",
    );
    let endpoint = h.ui.cascade.by_id[&self_id];
    let chrome = h.ui.forest.trees[endpoint.layer]
        .chrome(endpoint.node)
        .expect("disabled button chrome");
    let ShapeBrush::Solid(actual_fill) = chrome.fill else {
        panic!("disabled button must retain its solid test fill");
    };
    assert_eq!(
        actual_fill,
        ColorF16::from(disabled_fill),
        "fresh self-disable must pick disabled visuals before cascade catches up",
    );
}

/// The theme accessors' sharing contract, which is the whole point of
/// storing it behind an `Rc`: reads hand back a handle so the widgets
/// that need a bundle across a `&mut Ui` reborrow pay a refcount bump
/// rather than copying one; writes are copy-on-write, so a live handle
/// keeps the values it was taken with.
///
/// The `ptr_eq` assertion is the load-bearing one. If `Ui::theme` ever
/// went back to returning `&Theme`, every `ui.theme().clone()` call site
/// in the crate would still compile — and silently deep-copy ~9 KB of
/// bundles per widget per frame instead.
#[test]
fn theme_reads_share_and_writes_copy_on_write() {
    use crate::widgets::theme::Theme;
    use std::rc::Rc;

    let mut h = UiHarness::new(SURFACE);
    let clear = Color::rgb(0.25, 0.5, 0.75);
    h.ui.theme_mut().window_clear = clear;

    let handle = h.ui.theme().clone();
    assert!(
        Rc::ptr_eq(&handle, h.ui.theme()),
        "a theme read must hand back the same allocation, not a copy",
    );

    // Write with the handle alive: the `Ui` moves, the handle does not.
    let recolored = Color::rgb(0.1, 0.2, 0.3);
    h.ui.theme_mut().window_clear = recolored;
    assert_eq!(h.ui.theme().window_clear, recolored);
    assert_eq!(
        handle.window_clear, clear,
        "an outstanding handle must keep the values it was taken with",
    );
    assert!(
        !Rc::ptr_eq(&handle, h.ui.theme()),
        "the copy-on-write split must give the `Ui` a fresh allocation",
    );

    // With the handle dropped, the next write mutates in place.
    drop(handle);
    let before = Rc::as_ptr(h.ui.theme());
    h.ui.theme_mut().window_clear = clear;
    assert_eq!(
        Rc::as_ptr(h.ui.theme()),
        before,
        "an unshared theme must be written in place, with no copy",
    );

    // `set_theme` takes the handle, so swapping whole themes is a move.
    let swapped: Rc<Theme> = Rc::new(Theme::default());
    let swapped_ptr = Rc::as_ptr(&swapped);
    h.ui.set_theme(swapped);
    assert_eq!(Rc::as_ptr(h.ui.theme()), swapped_ptr);
}
