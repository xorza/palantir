use crate::host::harness::Harness;
use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;
use crate::internals::panic_probe;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::ui::Ui;
use crate::widget_core::click_outside::ClickOutside;
use crate::widget_core::configure::Configure;
use crate::widget_core::overlay_response::OverlayClose;
use crate::widgets::button::Button;
use crate::widgets::modal::Modal;
use crate::widgets::popup::Popup;
use glam::{UVec2, Vec2};

#[test]
fn explicit_zero_padding_and_minimum_override_card_theme() {
    let mut h = Harness::mono(UVec2::new(400, 300));
    let root_id = WidgetId::from_hash("modal-explicit-zero");
    h.frame(|ui| {
        Modal::new()
            .id(root_id)
            .chrome(Chrome::NONE)
            .padding(Spacing::ZERO)
            .min_size(Size::ZERO)
            .show(ui, |_, _| {});
    });

    let panel_id = root_id.with("panel");
    let panel = h.node_of(panel_id).expect("modal panel node");
    assert_eq!(panel.plane.layer, Layer::Modal);
    let tree = h.ui().tree(Layer::Modal);
    assert_eq!(
        tree.records.layout()[panel.node.idx()].padding,
        Spacing::ZERO
    );
    assert_eq!(tree.bounds(panel.node).min_size, Size::ZERO);
}

/// A modal takes no placement, so the layer default makes the backdrop cover the screen; the root paints the scrim.
#[test]
fn the_backdrop_root_covers_the_whole_surface_in_the_scrim() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    let id = WidgetId::from_hash("modal-full-surface");
    let custom = RgbaF32::srgba(0.2, 0.4, 0.6, 0.5);
    for explicit in [None, Some(custom)] {
        let mut h = Harness::mono(SURFACE);
        let themed = h.ui().theme().modal.scrim;
        h.frame(|ui| {
            let modal = Modal::new().id(id);
            match explicit {
                Some(c) => modal.scrim(c),
                None => modal,
            }
            .show(ui, |_, _| {});
        });

        let backdrop = h.node_of(id).expect("modal backdrop recorded");
        assert_eq!(backdrop.plane.layer, Layer::Modal);
        assert_eq!(
            h.ui().layout(Layer::Modal).rect[backdrop.node.idx()],
            Rect::new(0.0, 0.0, SURFACE.x as f32, SURFACE.y as f32),
        );
        let scrim = h
            .ui()
            .tree(Layer::Modal)
            .chrome(backdrop.node)
            .expect("the backdrop paints a scrim")
            .fill;
        let want = explicit.unwrap_or(themed).to_f16();
        assert!(
            matches!(scrim, ShapeBrush::Solid(fill) if fill == want),
            "explicit {explicit:?}: scrim {scrim:?}, want {want:?}",
        );
    }
}

/// What one frame of [`a_backdrop_click_or_escape_dismisses_unless_blocked`] reports.
#[derive(Clone, Copy, Debug)]
struct BackdropPass {
    under_clicked: bool,
    dismissed: bool,
}

/// A click on the backdrop and Esc each dismiss the modal under `Dismiss`, the default, and
/// neither does under `Block`; the button under it hears nothing either way, as the backdrop
/// covers the window and takes every pointer. `PassThrough` is no modal and panics.
#[test]
fn a_backdrop_click_or_escape_dismisses_unless_blocked() {
    for (click_outside, dismisses) in [(ClickOutside::Dismiss, true), (ClickOutside::Block, false)]
    {
        for by_escape in [false, true] {
            let under = WidgetId::from_hash("under-the-modal");
            let mut record = |ui: &mut Ui| BackdropPass {
                under_clicked: Button::new().id(under).label("under").show(ui).clicked(),
                dismissed: Modal::new()
                    .id(WidgetId::from_hash("modal"))
                    .click_outside(click_outside)
                    .show(ui, |_, _| {})
                    .close
                    == Some(OverlayClose::Dismissed),
            };
            let mut h = Harness::mono(UVec2::new(400, 300));
            h.frame_value(&mut record);
            if by_escape {
                h.key(Key::Escape);
            } else {
                let at = h.center_of(under);
                h.click(at);
            }
            let pass = h.frame_value(&mut record);
            let case = format!("{click_outside:?}, escape {by_escape}");
            assert_eq!(pass.dismissed, dismisses, "{case}");
            assert!(
                !pass.under_clicked,
                "{case}: the button under hears nothing"
            );
        }
    }
    panic_probe::assert_panics_with("a modal blocks the layers below it", || {
        Modal::new().click_outside(ClickOutside::PassThrough)
    });
}

/// A modal paints above every popup, so it must also hear Escape, which holds
/// only while its scope outranks the popup's. The no-popup control separates
/// "layer ordering works" from "Escape works".
#[test]
fn modal_hears_escape_even_while_a_popup_below_holds_keyboard_claim() {
    fn escape_dismisses(with_popup: bool) -> bool {
        const SURFACE: UVec2 = UVec2::new(400, 300);
        let scene = |ui: &mut Ui| {
            if with_popup {
                Popup::new()
                    .anchor(Anchor::at_point(Vec2::ZERO))
                    .id(WidgetId::from_hash("under-modal"))
                    .show(ui, |_ui, _handle| {});
            }
            Modal::new()
                .id(WidgetId::from_hash("modal-escape"))
                .show(ui, |_, _| {})
                .close
                == Some(OverlayClose::Dismissed)
        };

        // Two frames: the wake-gate parks a press whose shortcut nobody watched yet.
        let mut h = Harness::mono(SURFACE);
        h.frame(|ui| {
            scene(ui);
        });
        h.key(Key::Escape);
        h.frame_value(scene)
    }

    assert!(
        escape_dismisses(false),
        "control: a modal with no popup open must dismiss on Escape",
    );
    assert!(
        escape_dismisses(true),
        "a popup's keyboard capture must not silence the modal above it",
    );
}

/// A dismissed modal hands both streams back on the next frame; a dismissing
/// frame's claim could otherwise swallow the click landing where it was.
/// This does not pin `Ui::withdraw_input_scope` (a second record pass already
/// drops the claim; `release` is pinned in `input::tests::keyboard`).
#[test]
fn a_dismissed_modal_stops_owning_input_on_the_very_next_frame() {
    use crate::input::watch::{KeyboardWake, PointerWake};
    const SURFACE: UVec2 = UVec2::new(400, 300);

    let scene = |ui: &mut Ui, open: &mut bool| {
        ui.watch_pointer(PointerWake::BUTTONS);
        ui.watch_keyboard(KeyboardWake::KEY);
        if *open {
            *open = Modal::new()
                .id(WidgetId::from_hash("closing-modal"))
                .show(ui, |_, _| {})
                .close
                != Some(OverlayClose::Dismissed);
        }
    };

    let mut h = Harness::mono(SURFACE);
    let mut open = true;
    h.frame(|ui| scene(ui, &mut open));

    h.key(Key::Escape);
    h.frame(|ui| scene(ui, &mut open));
    assert!(!open, "Escape must dismiss the modal");

    h.press(Vec2::new(20.0, 20.0), PointerButton::Left);
    h.key(Key::Char('a'));
    let [pointer, keyboard] = h.frame_value(|ui| {
        scene(ui, &mut open);
        [ui.pointer_events().len(), ui.keyboard_events().len()]
    });
    assert_eq!(pointer, 1, "the dismissed modal still held the pointer");
    assert_eq!(keyboard, 1, "the dismissed modal still held the keyboard");
}

/// Exactly one overlay may act on a given Escape: ownership resolves topmost-first.
#[test]
fn escape_closes_only_the_topmost_overlay() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    #[derive(Debug)]
    struct Closed {
        popup: bool,
        modal: bool,
    }
    let scene = |ui: &mut Ui| Closed {
        popup: Popup::new()
            .anchor(Anchor::at_point(Vec2::ZERO))
            .id(WidgetId::from_hash("under-modal"))
            .show(ui, |_ui, _handle| {})
            .close
            == Some(OverlayClose::Dismissed),
        modal: Modal::new()
            .id(WidgetId::from_hash("over-popup"))
            .show(ui, |_, _| {})
            .close
            == Some(OverlayClose::Dismissed),
    };

    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        scene(ui);
    });
    h.key(Key::Escape);
    let closed = h.frame_value(scene);

    assert!(closed.modal, "the topmost overlay must take the Escape");
    assert!(
        !closed.popup,
        "the popup beneath the modal must not also consume it",
    );
}

/// The stock `modal.min_width` (280) is a default, so an authored `max_size` below it wins.
#[test]
fn an_authored_max_below_the_themed_min_width_wins() {
    let mut h = Harness::mono(UVec2::new(400, 300));
    let root_id = WidgetId::from_hash("narrow-modal");
    assert!(
        h.ui().theme().modal.min_width > 240.0,
        "fixture: the stock floor is above 240"
    );
    h.prime(2, |ui| {
        Modal::new()
            .id(root_id)
            .max_size((240.0, 400.0))
            .show(ui, |_, _| {});
    });
    let panel = h.arranged(root_id.with("panel"));
    assert_eq!(panel.unwrap().size.w, 240.0);
}

/// `opened` marks the first frame an overlay shows: a modal's and a popup's, again after a frame
/// it was not recorded in, and on no frame between.
#[test]
fn opened_marks_the_first_frame_an_overlay_shows() {
    let mut h = Harness::mono(UVec2::new(400, 300));
    let modal = WidgetId::from_hash("modal-opened");
    let popup = WidgetId::from_hash("popup-opened");
    let show = |h: &mut Harness, shown: bool| {
        h.frame_value(|ui| {
            shown.then(|| {
                let modal = Modal::new().id(modal).show(ui, |_, _| {}).opened;
                let popup = Popup::new()
                    .anchor(Anchor::at_point(Vec2::new(10.0, 10.0)))
                    .id(popup)
                    .show(ui, |_, _| {})
                    .opened;
                [modal, popup]
            })
        })
    };
    assert_eq!(show(&mut h, true), Some([true, true]));
    assert_eq!(show(&mut h, true), Some([false, false]));
    assert_eq!(show(&mut h, false), None);
    assert_eq!(show(&mut h, true), Some([true, true]), "shown again");
}

/// A caller that ends a modal or a popup itself, as a choice in its body does, gives the input
/// back with `close` on that frame: the key pressed on the next frame reaches the layer below. One
/// that only stops recording keeps the keys that next frame, so the key goes nowhere.
#[test]
fn closing_from_outside_gives_the_input_back_at_once() {
    use crate::input::watch::KeyboardWake;
    use crate::primitives::layout::anchor::Anchor;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Overlay {
        Modal,
        Popup,
    }
    let id = WidgetId::from_hash("ended-overlay");
    for overlay in [Overlay::Modal, Overlay::Popup] {
        for close in [true, false] {
            let mut h = Harness::mono(UVec2::new(400, 300));
            let mut open = true;
            let mut scene = |ui: &mut Ui, ending: bool| {
                ui.watch_keyboard(KeyboardWake::KEY);
                if open {
                    match overlay {
                        Overlay::Modal => {
                            Modal::new().id(id).show(ui, |_, _| {});
                        }
                        Overlay::Popup => {
                            Popup::new()
                                .anchor(Anchor::at_point(Vec2::new(20.0, 20.0)))
                                .id(id)
                                .show(ui, |_, _| {});
                        }
                    }
                    if ending {
                        open = false;
                        if close {
                            match overlay {
                                Overlay::Modal => Modal::close(ui, id),
                                Overlay::Popup => Popup::close(ui, id),
                            }
                        }
                    }
                }
                ui.keyboard_events().len()
            };
            h.frame(|ui| {
                scene(ui, false);
            });
            h.frame(|ui| {
                scene(ui, true);
            });
            h.key(Key::Char('a'));
            let heard = h.frame_value(|ui| scene(ui, false));
            assert_eq!(heard, usize::from(close), "{overlay:?}, close: {close}");
        }
    }
}
