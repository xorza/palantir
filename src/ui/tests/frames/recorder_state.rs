//! Theme sharing, and a subtree disabled between frames.

use crate::theme::color_scheme::ColorScheme;

use crate::host::harness::Harness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::tests::support::{self, SURFACE};
use crate::widget_core::configure::Configure;
use crate::widgets::{button::Button, panel::Panel};
use glam::Vec2;

/// `response_for`'s interaction half routes against the one-frame-stale cascade, so on the frame a subtree becomes disabled a widget could see `hovered`/`clicked` with `disabled == true`, which steady state never produces (disabled entries carry `Sense::NONE`).
#[test]
fn freshly_disabled_subtree_masks_stale_interactions() {
    use crate::shape::paint::lowered_brush::LoweredBrush;

    let target = WidgetId::from_hash("target");
    let mut h = Harness::mono(SURFACE);
    let run = |h: &mut Harness, disabled: bool| {
        h.frame_value(|ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("wrap"))
                .disabled(disabled)
                .show(ui, |ui| {
                    let resp = ui.response_for(target);
                    Button::new().label("hi").id(target).show(ui);
                    resp
                })
                .inner
        })
    };
    run(&mut h, false);
    h.hover(Vec2::new(10.0, 10.0));
    let enabled = run(&mut h, false);
    assert!(enabled.hovered(), "sanity: pointer hovers the button");
    assert!(!enabled.disabled);
    // Disable frame: the stale cascade still routes the hover; the read must mask it.
    let disabled = run(&mut h, true);
    assert!(disabled.disabled, "ancestor-disabled ORs in lag-free");
    assert!(
        !disabled.hovered(),
        "interactions must mask on the disable frame"
    );

    let self_id = WidgetId::from_hash("self-disabled");
    let disabled_fill = RgbaF32::srgb(0.8, 0.1, 0.2);
    let mut style = h.ui().theme().button.clone();
    style.unselected.disabled.chrome = Chrome::new(disabled_fill);
    let response = h.frame_value(|ui| {
        Button::new()
            .id(self_id)
            .label("disabled")
            .style(&style)
            .disabled(true)
            .show(ui)
    });
    assert!(
        response.disabled,
        "a self-disabled widget reports disabled on its own first frame, \
         before the cascade has seen it",
    );
    let endpoint = h.ui().input.arranged.cascade.endpoint(self_id).unwrap();
    let chrome = h.ui().forest.trees[endpoint.plane]
        .chrome(endpoint.node)
        .expect("disabled button chrome");
    let LoweredBrush::Solid(actual_fill) = chrome.fill else {
        panic!("disabled button must retain its solid test fill");
    };
    assert_eq!(
        actual_fill,
        disabled_fill.to_f16(),
        "fresh self-disable must pick disabled visuals before cascade catches up",
    );
}

/// The theme accessors' `Rc` sharing: reads hand back a handle (a refcount bump, not a copy) and
/// writes are copy-on-write, so a live handle keeps its values. The `ptr_eq` assertion matters: if
/// `Ui::theme` returned `&Theme`, every `ui.theme().clone()` would still compile and silently
/// deep-copy the theme per widget per frame.
#[test]
fn theme_reads_share_and_writes_copy_on_write() {
    use crate::theme::Theme;
    use std::rc::Rc;

    let mut h = Harness::mono(SURFACE);
    let clear = RgbaF32::srgb(0.25, 0.5, 0.75);
    h.ui_mut().theme_mut().window_bg = clear;

    let handle = Rc::clone(h.ui().theme());
    assert!(
        Rc::ptr_eq(&handle, h.ui().theme()),
        "a theme read must hand back the same allocation, not a copy",
    );

    // Write with the handle alive: the `Ui` moves, the handle doesn't.
    let recolored = RgbaF32::srgb(0.1, 0.2, 0.3);
    h.ui_mut().theme_mut().window_bg = recolored;
    assert_eq!(h.ui().theme().window_bg, recolored);
    assert_eq!(
        handle.window_bg, clear,
        "an outstanding handle must keep the values it was taken with",
    );
    assert!(
        !Rc::ptr_eq(&handle, h.ui().theme()),
        "the copy-on-write split must give the `Ui` a fresh allocation",
    );

    // With the handle dropped, the next write mutates in place.
    drop(handle);
    let before = Rc::as_ptr(h.ui().theme());
    h.ui_mut().theme_mut().window_bg = clear;
    assert_eq!(
        Rc::as_ptr(h.ui().theme()),
        before,
        "an unshared theme must be written in place, with no copy",
    );

    // `set_scheme_theme` takes the handle, so swapping themes is a move.
    let swapped: Rc<Theme> = Rc::new(support::still_theme(ColorScheme::Dark));
    let swapped_ptr = Rc::as_ptr(&swapped);
    h.ui_mut()
        .set_scheme_theme(ColorScheme::Dark, swapped)
        .unwrap();
    assert_eq!(Rc::as_ptr(h.ui().theme()), swapped_ptr);
}
