use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::reduced_backdrop::ReducedBackdrop;
use crate::scene::layer::Layer;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::shape::paint::lowered_material::LoweredMaterial;
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
            .show(ui, |_| {});
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

/// A modal takes no placement, so the layer default makes its barrier cover the screen; the root
/// paints the scrim, a chrome. The theme's dims in half-clear black with no material; a color
/// passed in dims in that color; a blurred scrim paints its fill over the window seen through its
/// material, a backdrop; and under reduced transparency a scrim that asks for nothing then drops
/// its material and keeps its fill half clear, while one that keeps its material reads the ground.
#[test]
fn the_barrier_root_covers_the_whole_surface_in_the_scrim() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    let id = WidgetId::from_hash("modal-full-surface");
    let custom = RgbaF32::srgba(0.2, 0.4, 0.6, 0.5);
    let blurred = Chrome::new(custom)
        .with_material(Material::blur(8.0).with_reduced_transparency(ReducedBackdrop::Nothing));
    let grounded = Chrome::new(custom).with_material(Material::blur(8.0));
    let themed = Chrome::new(RgbaF32::BLACK.with_alpha8(128));
    // What the scrim's material reads: a backdrop that captures, the ground, or none.
    for (label, explicit, reduced, fill, reads) in [
        ("themed", None, false, themed.paint.fill.clone(), None),
        (
            "a color",
            Some(Chrome::from(custom)),
            false,
            custom.into(),
            None,
        ),
        (
            "blurred",
            Some(blurred.clone()),
            false,
            custom.into(),
            Some(true),
        ),
        ("blurred, reduced", Some(blurred), true, custom.into(), None),
        (
            "grounded, reduced",
            Some(grounded),
            true,
            custom.into(),
            Some(false),
        ),
    ] {
        let mut h = Harness::mono(SURFACE);
        h.set_system_transparency_reduced(reduced);
        assert_eq!(h.ui().theme().modal.scrim, themed, "{label}");
        h.frame(|ui| {
            let modal = Modal::new().id(id);
            match &explicit {
                Some(scrim) => modal.scrim(scrim.clone()),
                None => modal,
            }
            .show(ui, |_| {});
        });

        let barrier = h.node_of(id).expect("modal barrier recorded");
        assert_eq!(barrier.plane.layer, Layer::Modal);
        assert_eq!(
            h.ui().plane_layout(Layer::Modal).rect[barrier.node.idx()],
            Rect::new(0.0, 0.0, SURFACE.x as f32, SURFACE.y as f32),
        );
        let row = h
            .ui()
            .tree(Layer::Modal)
            .chrome(barrier.node)
            .expect("the barrier paints a scrim");
        let want = fill.as_solid().expect("a solid scrim").to_f16();
        assert!(
            matches!(row.fill, LoweredBrush::Solid(fill) if fill == want),
            "{label}: scrim {:?}, want {want:?}",
            row.fill,
        );
        assert_eq!(
            row.material.map(LoweredMaterial::captures),
            reads,
            "{label}"
        );
    }
}

/// What one frame of [`a_barrier_click_or_escape_dismisses_unless_blocked`] reports.
#[derive(Clone, Copy, Debug)]
struct BarrierPass {
    under_clicked: bool,
    dismissed: bool,
}

/// A click on the barrier and Esc each dismiss the modal under `Dismiss`, the default, and
/// neither does under `Block`; the button under it hears nothing either way, as the barrier
/// covers the window and takes every pointer.
#[test]
fn a_barrier_click_or_escape_dismisses_unless_blocked() {
    for (click_outside, dismisses) in [(ClickOutside::Dismiss, true), (ClickOutside::Block, false)]
    {
        for by_escape in [false, true] {
            let under = WidgetId::from_hash("under-the-modal");
            let mut record = |ui: &mut Ui| BarrierPass {
                under_clicked: Button::new().id(under).label("under").show(ui).clicked(),
                dismissed: Modal::new()
                    .id(WidgetId::from_hash("modal"))
                    .click_outside(click_outside)
                    .show(ui, |_| {})
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
                    .show(ui, |_ui| {});
            }
            Modal::new()
                .id(WidgetId::from_hash("modal-escape"))
                .show(ui, |_| {})
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
    use crate::primitives::input::watch::{KeyboardWake, PointerWake};
    const SURFACE: UVec2 = UVec2::new(400, 300);

    let scene = |ui: &mut Ui, open: &mut bool| {
        ui.pointer_events(PointerWake::BUTTONS);
        ui.watch_keyboard(KeyboardWake::KEY);
        if *open {
            *open = Modal::new()
                .id(WidgetId::from_hash("closing-modal"))
                .show(ui, |_| {})
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
        [
            ui.pointer_events(PointerWake::BUTTONS).len(),
            ui.keyboard_events(KeyboardWake::NONE).len(),
        ]
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
            .show(ui, |_ui| {})
            .close
            == Some(OverlayClose::Dismissed),
        modal: Modal::new()
            .id(WidgetId::from_hash("over-popup"))
            .show(ui, |_| {})
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
            .show(ui, |_| {});
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
                let modal = Modal::new().id(modal).show(ui, |_| {}).opened;
                let popup = Popup::new()
                    .anchor(Anchor::at_point(Vec2::new(10.0, 10.0)))
                    .id(popup)
                    .show(ui, |_| {})
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
    use crate::primitives::input::watch::KeyboardWake;
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
                if open {
                    match overlay {
                        Overlay::Modal => {
                            Modal::new().id(id).show(ui, |_| {});
                        }
                        Overlay::Popup => {
                            Popup::new()
                                .anchor(Anchor::at_point(Vec2::new(20.0, 20.0)))
                                .id(id)
                                .show(ui, |_| {});
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
                ui.keyboard_events(KeyboardWake::KEY).len()
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

/// What one frame of [`an_opening_modal_dismisses_the_popups_open_before_it`] reports: how each
/// overlay closed, and whether the modal's button took a click.
#[derive(Clone, Copy, Debug, Default)]
struct CancelFrame {
    menu: Option<OverlayClose>,
    blocking: Option<OverlayClose>,
    inner: Option<OverlayClose>,
    container: Option<OverlayClose>,
    clicked: bool,
}

/// A modal that opens with no press, as on a close the window manager asks for, dismisses the
/// light-dismiss overlays open before it on the next frame, as their barriers would: a menu on
/// [`Layer::Menu`], above the modal's scrim, closes as `Dismissed`, and the first click on the
/// modal's button then reaches it. A popup that blocks outside presses is no light-dismiss one and
/// stays open beside it. A popup that opens inside the modal later stays open, and so does a popup
/// the modal is recorded inside, which would close the modal with it. A frame with no modal
/// opening dismisses nothing.
#[test]
fn an_opening_modal_dismisses_the_popups_open_before_it() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    let [menu, blocking, modal, inner, container, button] = [
        "cancel-menu",
        "cancel-blocking",
        "cancel-modal",
        "cancel-inner",
        "cancel-container",
        "cancel-button",
    ]
    .map(WidgetId::from_hash);
    let popup = |id: WidgetId, at: Vec2| Popup::new().anchor(Anchor::at_point(at)).id(id);
    let modal_body = |ui: &mut Ui, out: &mut CancelFrame, inner_open: bool| {
        Modal::new().id(modal).show(ui, |ui| {
            out.clicked = Button::new()
                .id(button)
                .label("Save")
                .show(ui)
                .left
                .clicked();
            if inner_open {
                out.inner = popup(inner, Vec2::new(200.0, 200.0)).show(ui, |_| {}).close;
            }
        });
    };

    // A menu open above where the modal opens, whose caller stops showing it once it closes.
    let mut h = Harness::mono(SURFACE);
    let mut menu_open = true;
    let mut scene = |ui: &mut Ui, modal_open: bool, inner_open: bool| {
        let mut out = CancelFrame::default();
        if menu_open {
            out.menu = popup(menu, Vec2::new(300.0, 10.0))
                .layer(Layer::Menu)
                .show(ui, |_| {})
                .close;
            menu_open = out.menu.is_none();
        }
        if !modal_open || !inner_open {
            out.blocking = popup(blocking, Vec2::new(10.0, 260.0))
                .click_outside(ClickOutside::Block)
                .show(ui, |_| {})
                .close;
        }
        if modal_open {
            modal_body(ui, &mut out, inner_open);
        }
        out
    };
    for _ in 0..2 {
        let quiet = h.frame_value(|ui| scene(ui, false, false));
        assert_eq!(quiet.menu, None, "no modal, no cancel");
        assert_eq!(quiet.blocking, None);
    }
    // The modal opens with no input: its frame takes a second pass, which closes the menu.
    let mut passes = Vec::new();
    h.frame(|ui| passes.push(scene(ui, true, false)));
    let [first, second] = passes[..] else {
        panic!("the modal's frame records twice: {passes:?}");
    };
    assert_eq!(first.menu, None, "the pass the modal opens in");
    assert_eq!(
        second.menu,
        Some(OverlayClose::Dismissed),
        "closed in the modal's own frame"
    );
    assert_eq!(second.blocking, None, "a popup that blocks stays");
    assert_eq!(
        h.ui().response_for(menu).rect,
        Some(Rect::new(300.0, 10.0, 0.0, 0.0)),
        "collapsed at its anchor, so it paints nothing"
    );
    h.click(button);
    assert!(
        h.frame_value(|ui| scene(ui, true, false)).clicked,
        "the first click reaches the modal"
    );
    h.frame(|ui| {
        scene(ui, true, true);
    });
    let inside = h.frame_value(|ui| scene(ui, true, true));
    assert_eq!(inside.inner, None, "a popup inside the modal stays open");

    // A modal recorded ahead of the menu closes it in its one pass: no second pass, and no
    // barrier, as the menu records collapsed. With no overlay open, one pass too.
    for menu_open in [true, false] {
        let mut h = Harness::mono(SURFACE);
        let scene = |ui: &mut Ui, modal_open: bool, menu_shown: bool| {
            let mut out = CancelFrame::default();
            if modal_open {
                modal_body(ui, &mut out, false);
            }
            if menu_shown {
                out.menu = popup(menu, Vec2::new(300.0, 10.0))
                    .layer(Layer::Menu)
                    .show(ui, |_| {})
                    .close;
            }
            out
        };
        h.prime(2, |ui| {
            scene(ui, false, menu_open);
        });
        let mut passes = Vec::new();
        h.frame(|ui| passes.push(scene(ui, true, menu_open)));
        let [only] = passes[..] else {
            panic!("menu open {menu_open}: one pass: {passes:?}");
        };
        let want = menu_open.then_some(OverlayClose::Dismissed);
        assert_eq!(only.menu, want, "menu open {menu_open}");
    }

    // The focus on a row of the menu goes to the modal's first Tab stop, in either order.
    let row = WidgetId::from_hash("cancel-row");
    for modal_first in [true, false] {
        let mut h = Harness::mono(SURFACE);
        let mut menu_open = true;
        let mut scene = |ui: &mut Ui, modal_open: bool| {
            let mut out = CancelFrame::default();
            if modal_open && modal_first {
                modal_body(ui, &mut out, false);
            }
            if menu_open {
                menu_open = popup(menu, Vec2::new(300.0, 10.0))
                    .layer(Layer::Menu)
                    .show(ui, |ui| {
                        Button::new().id(row).label("Row").show(ui);
                    })
                    .close
                    .is_none();
            }
            if modal_open && !modal_first {
                modal_body(ui, &mut out, false);
            }
        };
        h.prime(2, |ui| scene(ui, false));
        h.ui_mut().set_focus(row);
        h.frame(|ui| scene(ui, false));
        assert_eq!(h.ui().focus(), Some(row), "modal first: {modal_first}");
        h.frame(|ui| scene(ui, true));
        assert_eq!(h.ui().focus(), Some(button), "modal first: {modal_first}");
    }

    // A modal recorded inside a popup's body keeps that popup.
    let mut h = Harness::mono(SURFACE);
    let nested = |ui: &mut Ui, modal_open: bool| {
        let mut out = CancelFrame::default();
        out.container = popup(container, Vec2::new(10.0, 10.0))
            .show(ui, |ui| {
                if modal_open {
                    modal_body(ui, &mut out, false);
                }
            })
            .close;
        out
    };
    h.frame(|ui| {
        nested(ui, false);
    });
    h.frame(|ui| {
        nested(ui, true);
    });
    assert_eq!(h.frame_value(|ui| nested(ui, true)).container, None);
}

/// A modal that first records in a frame's last pass arms its cancel past that pass, so the frame
/// asks for the next one, whose first pass closes the menu; a quiet frame asks for none.
#[test]
fn a_modal_first_recorded_in_the_last_pass_asks_the_next_frame() {
    const SURFACE: UVec2 = UVec2::new(400, 300);
    let [menu, modal] = ["late-menu", "late-modal"].map(WidgetId::from_hash);
    let mut h = Harness::mono(SURFACE);
    let mut menu_open = true;
    // How the menu closed on this pass, `None` while it stays open or is gone.
    let mut scene = |ui: &mut Ui, modal_open: bool| {
        let mut close = None;
        if menu_open {
            close = Popup::new()
                .anchor(Anchor::at_point(Vec2::new(300.0, 10.0)))
                .id(menu)
                .layer(Layer::Menu)
                .show(ui, |_| {})
                .close;
            menu_open = close.is_none();
        }
        if modal_open {
            Modal::new().id(modal).show(ui, |_| {});
        }
        close
    };
    for _ in 0..2 {
        h.frame(|ui| {
            scene(ui, false);
        });
    }
    let quiet = h.step(|ui| {
        scene(ui, false);
    });
    assert!(!quiet.repaint_requested, "a quiet frame");

    // Pass A asks a second pass and shows no modal; pass B shows it first.
    let mut closes = Vec::new();
    let report = h.step(|ui| {
        if closes.is_empty() {
            ui.request_relayout();
        }
        closes.push(scene(ui, !closes.is_empty()));
    });
    assert_eq!(closes, [None, None], "both passes keep the menu");
    assert!(report.repaint_requested, "the cancel needs the next frame");
    closes.clear();
    h.step(|ui| closes.push(scene(ui, true)));
    assert_eq!(
        closes[0],
        Some(OverlayClose::Dismissed),
        "the next frame's first pass"
    );
}
