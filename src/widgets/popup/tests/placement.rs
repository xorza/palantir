//! Where the body lands: sizing, the upward flip near an edge, and
//! stability across frames.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::popup::tests::support::SURFACE;
use glam::{UVec2, Vec2};

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
        let popup_tree = h.ui.tree(Layer::Popup);
        let body_root = popup_tree.roots[1].first_node.idx();
        let body_rect = h.ui.layout(Layer::Popup).rect[body_root];
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

    let popup_tree = h.ui.tree(Layer::Popup);
    let body_root = popup_tree.roots[1].first_node.idx();
    let body_rect = h.ui.layout(Layer::Popup).rect[body_root];
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
    let body_root = h.ui.tree(Layer::Popup).roots[1].first_node.idx();
    let layout_min = h.ui.layout(Layer::Popup).rect[body_root].min;
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
    let viewport_id = WidgetId::from_hash("popup-scroll").with("viewport");
    let viewport =
        h.ui.cascade()
            .endpoint(viewport_id)
            .expect("popup scroll viewport endpoint");
    assert_eq!(viewport.layer, Layer::Popup);
    assert_eq!(h.ui.scroll_content(viewport_id), Size::new(80.0, 300.0));
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
