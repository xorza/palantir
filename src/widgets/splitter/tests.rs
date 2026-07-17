//! Splitter divider drag: pointer→ratio mapping through last frame's
//! arranged extent, clamping at explicit and content-driven stops,
//! the resulting pane re-layout, and the resize-cursor request.

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::layer::Layer;
use crate::input::InputEvent;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::frame::Frame;
use crate::widgets::splitter::{SplitHalf, Splitter, pointer_to_ratio, sanitize_ratio};
use crate::widgets::theme::splitter::SplitterTheme;
use crate::window::CursorIcon;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(500, 300);

fn split_id() -> WidgetId {
    WidgetId::from_hash("split")
}

/// One frame: a 401×100 horizontal splitter at the surface origin.
/// Default theme reserves the 1 px rule, so the free span is 400 —
/// seam center at x = ratio · 400 + 0.5, with the 6 px grab bar
/// straddling it. Tests run two warm-up frames before interacting so
/// the divider has arranged geometry for hit-testing.
fn frame_with(ui: &mut Ui, ratio: &mut f32) -> usize {
    let mut passes = 0;
    ui.run_at_acked(SURFACE, |ui| {
        passes += 1;
        Splitter::horizontal(ratio)
            .id(split_id())
            .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
            .min_pane(50.0)
            .show(ui, |_, _| {});
    });
    passes
}

#[test]
fn divider_drag_maps_pointer_to_ratio_without_relayout() {
    let mut ui = Ui::for_test();
    let mut ratio = 0.5;
    frame_with(&mut ui, &mut ratio);
    frame_with(&mut ui, &mut ratio);

    // ratio 0.5 → first pane [0, 200), rule [200, 201), grab bar
    // [197.5, 203.5). Press the seam center and drag 100 px right:
    // pointer 300.5 → first = 300 → 0.75.
    ui.press_at(Vec2::new(200.5, 50.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.5, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert!(
        (ratio - 0.75).abs() < 1e-6,
        "pointer 300.5 over span 400 → 0.75, got {ratio}"
    );

    // A later drag movement records once. Layout follows the current
    // pointer immediately, while the caller still receives the prior
    // arranged ratio until the next record.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(999.0, 50.0)));
    assert_eq!(frame_with(&mut ui, &mut ratio), 1);
    assert!(
        (ratio - 0.75).abs() < 1e-6,
        "model holds the prior arranged ratio for one record, got {ratio}"
    );
    let first = ui.node_for_widget_id(split_id().with("first"));
    let rect = ui.layout[Layer::Main].rect[first.idx()];
    assert!(
        (rect.size.w - 350.0).abs() < 0.5,
        "min_pane(50) stops the current layout at 350 px, got {}",
        rect.size.w
    );

    ui.on_input(InputEvent::PointerMoved(Vec2::new(998.0, 50.0)));
    assert_eq!(frame_with(&mut ui, &mut ratio), 1);
    assert!(
        (ratio - 0.875).abs() < 1e-6,
        "the next record writes back the arranged 350/400 ratio, got {ratio}"
    );

    // Release ends the gesture; further pointer motion leaves the
    // ratio alone.
    ui.release_left();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(100.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert!(
        (ratio - 0.875).abs() < 1e-6,
        "ratio holds after release, got {ratio}"
    );
}

#[test]
fn divider_and_pane_stop_together_when_content_is_rigid() {
    for (horizontal, rigid_half, expected_ratio) in [
        (true, SplitHalf::First, 0.45),
        (true, SplitHalf::Second, 0.55),
        (false, SplitHalf::First, 0.45),
        (false, SplitHalf::Second, 0.55),
    ] {
        let mut ui = Ui::for_test();
        let mut ratio = 0.5;
        let frame = |ui: &mut Ui, ratio: &mut f32| {
            let mut passes = 0;
            ui.run_at_acked(SURFACE, |ui| {
                passes += 1;
                let splitter = if horizontal {
                    Splitter::horizontal(ratio)
                } else {
                    Splitter::vertical(ratio)
                };
                splitter
                    .id(split_id())
                    .size(if horizontal {
                        (Sizing::fixed(401.0), Sizing::fixed(100.0))
                    } else {
                        (Sizing::fixed(100.0), Sizing::fixed(401.0))
                    })
                    .min_pane(50.0)
                    .show(ui, |ui, half| {
                        if half == rigid_half {
                            Frame::new()
                                .id(split_id().with("rigid"))
                                .size(if horizontal {
                                    (Sizing::fixed(180.0), Sizing::FILL)
                                } else {
                                    (Sizing::FILL, Sizing::fixed(180.0))
                                })
                                .show(ui);
                        }
                    });
            });
            passes
        };
        frame(&mut ui, &mut ratio);
        frame(&mut ui, &mut ratio);

        ui.press_at(if horizontal {
            Vec2::new(200.5, 50.0)
        } else {
            Vec2::new(50.0, 200.5)
        });
        let activation_main = 210.5;
        ui.on_input(InputEvent::PointerMoved(if horizontal {
            Vec2::new(activation_main, 50.0)
        } else {
            Vec2::new(50.0, activation_main)
        }));
        frame(&mut ui, &mut ratio);

        let pointer_main = if rigid_half == SplitHalf::First {
            -100.0
        } else {
            500.0
        };
        ui.on_input(InputEvent::PointerMoved(if horizontal {
            Vec2::new(pointer_main, 50.0)
        } else {
            Vec2::new(50.0, pointer_main)
        }));
        assert_eq!(
            frame(&mut ui, &mut ratio),
            1,
            "active drag movement must not request a second layout"
        );

        assert!(
            (ratio - 0.525).abs() < 1e-6,
            "model keeps the prior arranged ratio for one record"
        );
        let shrinking = ui.node_for_widget_id(split_id().with(match rigid_half {
            SplitHalf::First => "first",
            SplitHalf::Second => "second",
        }));
        let shrinking_rect = ui.layout[Layer::Main].rect[shrinking.idx()];
        assert_eq!(
            if horizontal {
                shrinking_rect.size.w
            } else {
                shrinking_rect.size.h
            },
            180.0,
            "{rigid_half:?} pane stops at its rigid content floor"
        );
        let rigid = ui.node_for_widget_id(split_id().with("rigid"));
        let rigid_rect = ui.layout[Layer::Main].rect[rigid.idx()];
        assert_eq!(
            if horizontal {
                rigid_rect.size.w
            } else {
                rigid_rect.size.h
            },
            180.0,
            "rigid content remains laid out"
        );
        let first = ui.node_for_widget_id(split_id().with("first"));
        let first_rect = ui.layout[Layer::Main].rect[first.idx()];
        let divider_rect = ui
            .response_for(split_id().with("divider"))
            .rect
            .expect("divider arranged");
        let divider_center = if horizontal {
            divider_rect.center().x
        } else {
            divider_rect.center().y
        };
        let first_edge = if horizontal {
            first_rect.max().x
        } else {
            first_rect.max().y
        };
        assert_eq!(
            divider_center,
            first_edge + 0.5,
            "{rigid_half:?} divider center stays on the arranged pane edge"
        );

        let next_pointer = if rigid_half == SplitHalf::First {
            -101.0
        } else {
            501.0
        };
        ui.on_input(InputEvent::PointerMoved(if horizontal {
            Vec2::new(next_pointer, 50.0)
        } else {
            Vec2::new(50.0, next_pointer)
        }));
        assert_eq!(frame(&mut ui, &mut ratio), 1);
        assert!(
            (ratio - expected_ratio).abs() < 1e-6,
            "{rigid_half:?} next record writes back its content floor"
        );
    }
}

#[test]
fn divider_requests_the_resize_cursor() {
    let mut ui = Ui::for_test();
    let mut ratio = 0.5;
    frame_with(&mut ui, &mut ratio);
    frame_with(&mut ui, &mut ratio);
    assert_eq!(
        ui.window_mailbox.cursor,
        CursorIcon::Default,
        "idle frame keeps the arrow"
    );

    // Hovering the grab bar ([197.5, 203.5) at ratio 0.5) requests the
    // horizontal-resize cursor.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(200.5, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert_eq!(
        ui.window_mailbox.cursor,
        CursorIcon::EwResize,
        "hover shows resize"
    );

    // Mid-drag the pointer leaves the thin bar; the cursor must hold
    // until release (drag-first, since `hovered` is capture-gated).
    ui.press_at(Vec2::new(200.5, 50.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(320.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert_eq!(
        ui.window_mailbox.cursor,
        CursorIcon::EwResize,
        "drag holds resize off-bar"
    );

    // Release with the pointer over a pane: the per-record-pass reset
    // returns the arrow because nothing re-requests.
    ui.release_left();
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
    frame_with(&mut ui, &mut ratio);
    assert_eq!(
        ui.window_mailbox.cursor,
        CursorIcon::Default,
        "leave resets to the arrow"
    );

    // A vertical splitter's divider asks for the other axis.
    let mut ui = Ui::for_test();
    let mut ratio = 0.5;
    let frame = |ui: &mut Ui, ratio: &mut f32| {
        ui.run_at_acked(SURFACE, |ui| {
            Splitter::vertical(ratio)
                .id(split_id())
                .size((Sizing::fixed(100.0), Sizing::fixed(201.0)))
                .show(ui, |_, _| {});
        });
    };
    frame(&mut ui, &mut ratio);
    frame(&mut ui, &mut ratio);
    // Free span 200 at ratio 0.5 → grab bar rows [97.5, 103.5).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 100.5)));
    frame(&mut ui, &mut ratio);
    assert_eq!(
        ui.window_mailbox.cursor,
        CursorIcon::NsResize,
        "column split resizes vertically"
    );

    for (thickness, rule_thickness) in [(3.0, 9.0), (6.0, 0.0)] {
        let mut ui = Ui::for_test();
        let mut ratio = 0.5;
        let frame = |ui: &mut Ui, ratio: &mut f32| {
            ui.run_at_acked(SURFACE, |ui| {
                let style = SplitterTheme {
                    thickness,
                    rule_thickness,
                    ..SplitterTheme::default()
                };
                Splitter::horizontal(ratio)
                    .id(split_id())
                    .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
                    .style(style)
                    .show(ui, |_, _| {});
            });
        };
        frame(&mut ui, &mut ratio);
        frame(&mut ui, &mut ratio);

        let first = ui.node_for_widget_id(split_id().with("first"));
        let first_rect = ui.layout[Layer::Main].rect[first.idx()];
        let divider_rect = ui
            .response_for(split_id().with("divider"))
            .rect
            .expect("divider arranged");
        assert_eq!(divider_rect.size.w, thickness);
        assert_eq!(
            divider_rect.center().x,
            first_rect.max().x + rule_thickness * 0.5,
            "grab bar stays centered whether narrower or wider than the rule"
        );
    }
}

#[test]
fn pointer_to_ratio_maps_center_edges_and_floors() {
    // extent 406, reserved 6 → span 400; seam center at
    // pointer, so pointer 203 → first = 200 → ratio 0.5.
    let cases = [
        // (pos, extent, reserved, min_pane, want)
        (203.0, 406.0, 6.0, 0.0, 0.5),
        (3.0, 406.0, 6.0, 0.0, 0.0),   // at the left stop
        (403.0, 406.0, 6.0, 0.0, 1.0), // at the right stop
        (-50.0, 406.0, 6.0, 0.0, 0.0), // past the ends clamps
        (999.0, 406.0, 6.0, 0.0, 1.0),
        (103.0, 406.0, 6.0, 0.0, 0.25),   // quarter point
        (10.0, 406.0, 6.0, 50.0, 0.125),  // min_pane floors first: 50/400
        (395.0, 406.0, 6.0, 50.0, 0.875), // …and second: 350/400
        (7.0, 406.0, 6.0, 300.0, 0.5),    // floors can't both fit → center
        (10.0, 4.0, 6.0, 0.0, 0.5),       // degenerate extent
    ];
    for (pos, extent, thickness, min_pane, want) in cases {
        let got = pointer_to_ratio(pos, extent, thickness, min_pane);
        assert!(
            (got - want).abs() < 1e-6,
            "p2r({pos},{extent},{thickness},{min_pane})={got} want {want}"
        );
    }
}

#[test]
fn sanitize_ratio_clamps_and_pins_non_finite() {
    assert_eq!(sanitize_ratio(0.3), 0.3);
    assert_eq!(sanitize_ratio(-0.2), 0.0);
    assert_eq!(sanitize_ratio(1.5), 1.0);
    assert_eq!(sanitize_ratio(f32::NAN), 0.5);
    assert_eq!(sanitize_ratio(f32::INFINITY), 0.5);
}

#[test]
fn endpoint_ratios_collapse_exactly_one_pane() {
    for (ratio, expected) in [(0.0, [0.0, 400.0]), (1.0, [400.0, 0.0])] {
        let mut ui = Ui::for_test();
        let mut ratio = ratio;
        ui.run_at_acked(SURFACE, |ui| {
            Splitter::horizontal(&mut ratio)
                .id(split_id())
                .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
                .show(ui, |_, _| {});
        });
        let first = ui.node_for_widget_id(split_id().with("first"));
        let second = ui.node_for_widget_id(split_id().with("second"));
        let rects = &ui.layout[Layer::Main].rect;
        assert_eq!(
            [rects[first.idx()].size.w, rects[second.idx()].size.w],
            expected,
            "ratio {ratio}",
        );
    }
}
