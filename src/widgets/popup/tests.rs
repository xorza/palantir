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

use crate::input::keyboard::Key;
use crate::input::pointer::PointerButton;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
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
        h.ui.forest.trees[Layer::Popup].records.len(),
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
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
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
        let popup_tree = &h.ui.forest.trees[Layer::Popup];
        let body_root = popup_tree.roots[1].first_node.idx();
        let body_rect = h.ui.layout[Layer::Popup].rect[body_root];
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
    let mut h = UiHarness::new(SURF);
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
    h.frame(scene);

    let popup_tree = &h.ui.forest.trees[Layer::Popup];
    let body_root = popup_tree.roots[1].first_node.idx();
    let body_rect = h.ui.layout[Layer::Popup].rect[body_root];
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
    let mut h = UiHarness::new(SURF);
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
    h.frame(scene);

    let flipped_min = Vec2::new(anchor.x, anchor.y - content.h); // (20, 80)
    let body_root = h.ui.forest.trees[Layer::Popup].roots[1].first_node.idx();
    let layout_min = h.ui.layout[Layer::Popup].rect[body_root].min;
    assert_eq!(layout_min, flipped_min, "layout sanity: popup flipped");

    // The cascade-backed response rect is what the encoder paints. It
    // must agree with the layout — a mismatch means the flip didn't
    // propagate to paint (the reported clipping bug).
    let painted_min =
        h.ui.response_for(body_id)
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
    let mut h = UiHarness::new(SURF);
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
    h.frame(scene);
    let first = body_rect(&h.ui);
    let viewport_id = WidgetId::from_hash("popup-scroll").with("__viewport");
    let viewport =
        h.ui.cascades
            .by_id
            .get(&viewport_id)
            .expect("popup scroll viewport endpoint");
    assert_eq!(viewport.layer, Layer::Popup);
    assert_eq!(
        h.ui.layout[Layer::Popup].scroll_content[viewport.node.idx()],
        Size::new(80.0, 300.0)
    );
    // Subsequent input frames must hit the same rect — no drift.
    for _ in 0..3 {
        h.move_to(Vec2::new(50.0, 50.0));
        h.frame(scene);
        assert_eq!(
            body_rect(&h.ui),
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
    let mut h = UiHarness::new(SURF);
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
    h.frame(scene);
    let first = body_rect_of(&h.ui);
    // Pretend an input arrived (cursor move over the popup).
    h.move_to(Vec2::new(50.0, 100.0));
    h.frame(scene);
    let second = body_rect_of(&h.ui);
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
        let mut h = UiHarness::new(EDGE_SURFACE);
        let body_id = WidgetId::from_hash("dynamic-popup");
        let frame = |h: &mut UiHarness, size: Size| {
            let mut passes = 0;
            h.frame(|ui| {
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
            h.ui.response_for(body_id)
                .rect
                .expect("popup body arranged")
        };

        let small = frame(&mut h, Size::new(80.0, 40.0));
        assert_eq!(
            small,
            Rect {
                min: small_min,
                size: Size::new(80.0, 40.0),
            },
        );

        let large = frame(&mut h, Size::new(160.0, 100.0));
        assert_eq!(
            large,
            Rect {
                min: large_min,
                size: Size::new(160.0, 100.0),
            },
        );

        let shrunk = frame(&mut h, Size::new(80.0, 40.0));
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

/// A text field inside a popup must be typeable.
///
/// It was not: `Popup::show` holds keyboard capture for its whole body, and
/// `TextEdit` drains the *uncaptured* stream, so before capture became
/// layer-ordered every keystroke into a popup-hosted field was discarded.
/// Nothing in the tree exercised the combination, so it went unnoticed.
///
/// This works because `Popup::show` calls `with_keyboard_claim` *outside*
/// `ui.layer(Layer::Popup, ..)`, registering the capture at `Layer::Main` —
/// so the body, one layer up, is not silenced by it. That is load-bearing:
/// moving the capture call inside the layer scope would put owner and body
/// on the same layer and silently break typing again, which is what this
/// test is here to catch.
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

    h.ime_commit("x");
    h.frame(|ui| scene(ui, &mut buf));

    assert_eq!(
        buf, "x",
        "the popup's keyboard capture must not swallow typing aimed at a \
         field inside its own body",
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
    h.ui.input.focused = Some(content);
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
    h.ui.input.focused = Some(WidgetId::from_hash("main-bg"));
    h.key(Key::Escape);
    let mut seen = 0usize;
    h.frame(|ui| {
        build(ui, false, &mut dismissed);
        seen = seen.max(ui.input.keyboard_events(Layer::Main).len());
    });
    assert_eq!(seen, 1, "the frame after dismissal must reach Main");
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
    use crate::input::keyboard::Key;
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
