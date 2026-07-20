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

use crate::input::InputEvent;
use crate::input::keyboard::Key;
use crate::input::pointer::PointerButton;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::Configure;
use crate::scene::layer::Layer;
use crate::widgets::panel::Panel;
use crate::widgets::popup::{ClickOutside, Popup};
use crate::{Sense, Ui};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 400);
const ANCHOR: Vec2 = Vec2::new(50.0, 50.0);
const BODY_W: f32 = 100.0;
const BODY_H: f32 = 60.0;

// `Ui::frame` re-runs the build closure when action input is pending,
// so we OR `dismissed` across passes — pass 1 sees the click, pass 2
// would otherwise overwrite with a fresh false.
fn record_body(ui: &mut Ui, config: ClickOutside, dismissed: &mut bool) {
    Panel::vstack()
        .id(WidgetId::from_hash("main-bg"))
        .size((Sizing::FILL, Sizing::FILL))
        .sense(Sense::CLICK)
        .show(ui, |ui| {
            let r = Popup::anchored_to(ANCHOR)
                .id(WidgetId::from_hash("test-popup"))
                .click_outside(config)
                .padding(4.0)
                .show(ui, |ui, _popup| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-content"))
                        .size((Sizing::fixed(100.0), Sizing::fixed(60.0)))
                        .show(ui, |_| {});
                });
            *dismissed |= r.dismissed;
        });
}

fn main_panel_clicked(ui: &Ui) -> bool {
    let main_id = WidgetId::from_hash("main-bg");
    ui.response_for(main_id).left.clicked()
}

#[test]
fn click_inside_popup_does_not_dismiss() {
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    let inside = Vec2::new(ANCHOR.x + BODY_W * 0.5, ANCHOR.y + BODY_H * 0.5);
    ui.click_at(inside);

    let mut dismissed = false;
    ui.run_at_without_baseline(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    assert!(!dismissed, "click inside body must not signal dismissal");
    assert!(
        !main_panel_clicked(&ui),
        "click inside body must not leak to Main"
    );
}

#[test]
fn click_outside_popup_dismisses_and_blocks_main() {
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    ui.click_at(Vec2::new(300.0, 300.0));

    let mut dismissed = false;
    ui.run_at_without_baseline(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    assert!(
        dismissed,
        "outside click with `Dismiss` must signal dismissal"
    );
    assert!(
        !main_panel_clicked(&ui),
        "outside click must be eaten by the popup eater, not leak to Main",
    );
}

#[test]
fn escape_dismisses_dismiss_popup_but_not_block() {
    let esc = || InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
        physical: Key::Escape,
    };

    // `Dismiss`: Esc folds into `dismissed`.
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    ui.on_input(esc());
    let mut dismissed = false;
    ui.run_at_without_baseline(SURFACE, |ui| {
        record_body(ui, ClickOutside::Dismiss, &mut dismissed);
    });
    assert!(dismissed, "Esc dismisses a `Dismiss` popup");

    // `Block`: Esc is ignored (stop-the-world prompts close only on the
    // host's terms).
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    ui.on_input(esc());
    let mut dismissed = false;
    ui.run_at_without_baseline(SURFACE, |ui| {
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
    let mut ui = Ui::for_test();
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
    ui.run_at(SURFACE, |ui| scene(ui, &mut open));
    ui.click_at(Vec2::new(300.0, 300.0));
    ui.run_at_without_baseline(SURFACE, |ui| scene(ui, &mut open));
    assert!(!open, "host flag must flip to false in pass 1");
    assert_eq!(
        ui.forest.trees[Layer::Popup].records.len(),
        0,
        "painted tree (pass 2) must contain no Popup-layer widgets",
    );
}

/// Pin popup-body sizing + anchor placement under each `Sizing` mode.
/// `Popup::show` measures against the full surface before resolving its
/// shared edge-aware position.
///
/// - `Hug` / `Fixed` bodies fit at the raw anchor with room to spare.
/// - `FILL` fills the full surface and the safety clamp
///   shifts it to `(0, 0)` — the body is the size of the surface and
///   can't sit at the anchor without overflowing.
#[test]
fn popup_body_sizing_matches_sizing_mode() {
    use crate::scene::layer::Layer;
    let anchor = Vec2::new(20.0, 30.0);
    let cases: &[(Sizing, Sizing, Size, Vec2)] = &[
        (Sizing::HUG, Sizing::HUG, Size::new(100.0, 60.0), anchor),
        (
            Sizing::FILL,
            Sizing::FILL,
            Size::new(SURFACE.x as f32, SURFACE.y as f32),
            Vec2::ZERO,
        ),
        (
            Sizing::fixed(80.0),
            Sizing::fixed(40.0),
            Size::new(80.0, 40.0),
            anchor,
        ),
    ];
    for &(sw, sh, expected_size, expected_min) in cases {
        let mut ui = Ui::for_test();
        ui.run_at_without_baseline(SURFACE, |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("main-bg"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Popup::anchored_to(anchor)
                        .id(WidgetId::from_hash("sized-popup"))
                        .padding(0.0)
                        .size((sw, sh))
                        .show(ui, |ui, _popup| {
                            Panel::vstack()
                                .id(WidgetId::from_hash("popup-content"))
                                .size((Sizing::fixed(100.0), Sizing::fixed(60.0)))
                                .show(ui, |_| {});
                        });
                });
        });
        let popup_tree = &ui.forest.trees[Layer::Popup];
        let body_root = popup_tree.roots[1].first_node.idx();
        let body_rect = ui.layout[Layer::Popup].rect[body_root];
        assert_eq!(
            body_rect.size, expected_size,
            "size=({:?},{:?}) → expected {:?}, got {:?}",
            sw, sh, expected_size, body_rect.size,
        );
        assert_eq!(
            body_rect.min, expected_min,
            "size=({:?},{:?}) → expected anchor {:?}, got {:?}",
            sw, sh, expected_min, body_rect.min,
        );
    }
}

/// A popup keeps its natural size and flips above a near-bottom anchor.
#[test]
fn popup_near_bottom_flips_upward() {
    use crate::scene::layer::Layer;
    const SURF: UVec2 = UVec2::new(400, 300);
    let anchor = Vec2::new(20.0, 280.0); // 20 px of room below.
    let content = Size::new(120.0, 200.0); // Body wants ~200 tall.
    let mut ui = Ui::for_test();
    let scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Popup::anchored_to(anchor)
                    .id(WidgetId::from_hash("flip-popup"))
                    .padding(0.0)
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui, _popup| {
                        Panel::vstack()
                            .id(WidgetId::from_hash("flip-content"))
                            .size((Sizing::fixed(content.w), Sizing::fixed(content.h)))
                            .show(ui, |_| {});
                    });
            });
    };
    ui.run_at_without_baseline(SURF, scene);

    let popup_tree = &ui.forest.trees[Layer::Popup];
    let body_root = popup_tree.roots[1].first_node.idx();
    let body_rect = ui.layout[Layer::Popup].rect[body_root];
    assert_eq!(
        body_rect.size, content,
        "body measured at full content size (anchor-independent available)",
    );
    // Flip upward: anchor.y − body.h = 280 − 200 = 80, well inside
    // the surface. The popup's top-left sits at `(anchor.x, 80)`.
    assert_eq!(
        body_rect.min,
        Vec2::new(anchor.x, anchor.y - content.h),
        "popup near bottom anchor flipped above the anchor",
    );
}

/// The placement policy participates in the cascade fingerprint, so the
/// painted position stays synchronized with layout.
#[test]
fn popup_flip_reaches_cascade_not_just_layout() {
    use crate::scene::layer::Layer;
    const SURF: UVec2 = UVec2::new(400, 300);
    let anchor = Vec2::new(20.0, 280.0); // near the bottom → must flip.
    let content = Size::new(120.0, 200.0);
    let body_id = WidgetId::from_hash("cascade-flip-popup");
    let mut ui = Ui::for_test();
    let scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Popup::anchored_to(anchor)
                    .id(body_id)
                    .padding(0.0)
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui, _popup| {
                        Panel::vstack()
                            .id(WidgetId::from_hash("cascade-flip-content"))
                            .size((Sizing::fixed(content.w), Sizing::fixed(content.h)))
                            .show(ui, |_| {});
                    });
            });
    };
    ui.run_at(SURF, scene);

    let flipped_min = Vec2::new(anchor.x, anchor.y - content.h); // (20, 80)
    let body_root = ui.forest.trees[Layer::Popup].roots[1].first_node.idx();
    let layout_min = ui.layout[Layer::Popup].rect[body_root].min;
    assert_eq!(layout_min, flipped_min, "layout sanity: popup flipped");

    // The cascade-backed response rect is what the encoder paints. It
    // must agree with the layout — a mismatch means the flip didn't
    // propagate to paint (the reported clipping bug).
    let painted_min = ui
        .response_for(body_id)
        .rect
        .expect("popup body has a cascade rect after the opening frame")
        .min;
    assert_eq!(
        painted_min, flipped_min,
        "painted (cascade) popup position must match the flipped layout, \
         not the stale pre-flip anchor",
    );
}

/// A popup containing [`crate::Scroll`] resolves at an edge in one frame.
#[test]
fn popup_with_scroll_settles_in_one_frame() {
    use crate::Scroll;
    const SURF: UVec2 = UVec2::new(400, 400);
    // Anchor near the right edge so any body-width change between
    // passes would drift the placement.
    let anchor = Vec2::new(380.0, 20.0);
    let mut ui = Ui::for_test();
    let scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Popup::anchored_to(anchor)
                    .id(WidgetId::from_hash("scroll-popup"))
                    .padding(0.0)
                    .size((Sizing::HUG, Sizing::HUG))
                    .max_size((f32::INFINITY, 100.0))
                    .show(ui, |ui, _| {
                        Scroll::vertical()
                            .id(WidgetId::from_hash("popup-scroll"))
                            .size((Sizing::HUG, Sizing::fill(1.0)))
                            .show(ui, |ui| {
                                Panel::vstack()
                                    .id(WidgetId::from_hash("scroll-content"))
                                    .size((Sizing::fixed(80.0), Sizing::fixed(300.0)))
                                    .show(ui, |_| {});
                            });
                    });
            });
    };
    let body_id = WidgetId::from_hash("scroll-popup");
    let body_rect = |ui: &Ui| {
        ui.response_for(body_id)
            .rect
            .expect("popup body has a rect")
    };
    ui.run_at(SURF, scene);
    let first = body_rect(&ui);
    // Subsequent input frames must hit the same rect — no drift.
    for _ in 0..3 {
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        ui.run_at(SURF, scene);
        assert_eq!(
            body_rect(&ui),
            first,
            "popup must hold its settled position from the opening frame on",
        );
    }
}

/// A flipped popup remains stable across input and idle frames.
#[test]
fn popup_placement_is_stable_across_frames() {
    const SURF: UVec2 = UVec2::new(400, 300);
    let anchor = Vec2::new(20.0, 280.0);
    let content = Size::new(120.0, 200.0);
    let mut ui = Ui::for_test();
    let scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("main-bg"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Popup::anchored_to(anchor)
                    .id(WidgetId::from_hash("stable-popup"))
                    .padding(0.0)
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui, _popup| {
                        Panel::vstack()
                            .id(WidgetId::from_hash("stable-content"))
                            .size((Sizing::fixed(content.w), Sizing::fixed(content.h)))
                            .show(ui, |_| {});
                    });
            });
    };
    let body_id = WidgetId::from_hash("stable-popup");
    let body_rect_of = |ui: &Ui| {
        ui.response_for(body_id)
            .rect
            .expect("popup body has an arranged rect after the opening frame")
    };
    ui.run_at(SURF, scene);
    let first = body_rect_of(&ui);
    // Pretend an input arrived (cursor move over the popup).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 100.0)));
    ui.run_at(SURF, scene);
    let second = body_rect_of(&ui);
    assert_eq!(
        first, second,
        "popup must not shift between opening frame and the next input-triggered frame",
    );
}

#[test]
fn dynamic_body_size_repositions_at_every_viewport_edge_without_settling() {
    const EDGE_SURFACE: UVec2 = UVec2::new(400, 300);

    #[derive(Clone, Copy, Debug)]
    enum Edge {
        Top,
        Right,
        Bottom,
        Left,
    }

    let cases = [
        (
            Edge::Top,
            Rect::new(150.0, 70.0, 100.0, 30.0),
            Vec2::new(150.0, 30.0),
            Vec2::new(150.0, 100.0),
        ),
        (
            Edge::Right,
            Rect::new(270.0, 130.0, 30.0, 40.0),
            Vec2::new(300.0, 130.0),
            Vec2::new(110.0, 130.0),
        ),
        (
            Edge::Bottom,
            Rect::new(150.0, 230.0, 100.0, 30.0),
            Vec2::new(150.0, 260.0),
            Vec2::new(150.0, 130.0),
        ),
        (
            Edge::Left,
            Rect::new(100.0, 130.0, 30.0, 40.0),
            Vec2::new(20.0, 130.0),
            Vec2::new(130.0, 130.0),
        ),
    ];

    for (edge, anchor, small_min, large_min) in cases {
        let mut ui = Ui::for_test();
        let body_id = WidgetId::from_hash("dynamic-popup");
        let frame = |ui: &mut Ui, size: Size| {
            let mut passes = 0;
            ui.run_at(EDGE_SURFACE, |ui| {
                passes += 1;
                let popup = match edge {
                    Edge::Top => Popup::above(anchor),
                    Edge::Right => Popup::right_of(anchor),
                    Edge::Bottom => Popup::below(anchor),
                    Edge::Left => Popup::left_of(anchor),
                };
                popup
                    .id(body_id)
                    .padding(0.0)
                    .background(Default::default())
                    .show(ui, |ui, _| {
                        Panel::vstack()
                            .id(WidgetId::from_hash("dynamic-content"))
                            .size((Sizing::fixed(size.w), Sizing::fixed(size.h)))
                            .show(ui, |_| {});
                    });
            });
            assert_eq!(passes, 1, "{edge:?} must converge in one pass");
            ui.response_for(body_id).rect.expect("popup body arranged")
        };

        let small = frame(&mut ui, Size::new(80.0, 40.0));
        assert_eq!(
            small,
            Rect {
                min: small_min,
                size: Size::new(80.0, 40.0),
            },
        );

        let large = frame(&mut ui, Size::new(160.0, 100.0));
        assert_eq!(
            large,
            Rect {
                min: large_min,
                size: Size::new(160.0, 100.0),
            },
        );

        let shrunk = frame(&mut ui, Size::new(80.0, 40.0));
        assert_eq!(shrunk, small, "{edge:?} shrink must reposition immediately");
    }
}

/// Pin: pointer gestures over the area outside the popup body must be
/// absorbed by the eater — not leak through to a `Main` widget below
/// that senses the same gesture. Earlier the eater only sensed
/// `CLICK`, so a graph canvas underneath would still receive scroll /
/// pinch / drag while the popup was open.
#[test]
fn outside_pointer_gestures_do_not_leak_to_main() {
    let mut ui = Ui::for_test();
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
    ui.run_at(SURFACE, scene);

    // Move pointer well outside the popup body, then send a scroll
    // + zoom + middle-drag burst.
    let outside = Vec2::new(300.0, 300.0);
    ui.on_input(InputEvent::PointerMoved(outside));
    ui.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 25.0)));
    ui.on_input(InputEvent::ScrollLines(Vec2::new(0.0, 3.0)));
    ui.on_input(InputEvent::Zoom(1.4));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Middle));
    ui.on_input(InputEvent::PointerMoved(outside + Vec2::new(40.0, 0.0)));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Middle));

    ui.run_at_without_baseline(SURFACE, scene);
    let bg = ui.response_for(bg_id);
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
    let mut ui = Ui::for_test();
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    ui.click_at(Vec2::new(300.0, 300.0));

    let mut dismissed = false;
    ui.run_at_without_baseline(SURFACE, |ui| {
        record_body(ui, ClickOutside::Block, &mut dismissed);
    });
    assert!(!dismissed, "`Block` mode must not signal dismissal");
    assert!(
        !main_panel_clicked(&ui),
        "`Block` mode must still eat the click — no leak to Main",
    );
}
