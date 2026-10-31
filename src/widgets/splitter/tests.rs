//! Splitter divider drag: pointer→ratio mapping, clamping at stops, pane re-layout, and the resize cursor.

use crate::host::harness::Harness;
use crate::host::harness::unit::Passes;
use crate::input::pointer::PointerButton;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::theme::splitter::SplitterTheme;
use crate::ui::frame_report::FrameProcessing;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::internals::ValueEdges;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::splitter::split_half::SplitHalf;
use crate::widgets::splitter::{Seam, Splitter, sanitize_ratio};
use crate::window::cursor_icon::CursorIcon;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(500, 300);

fn split_id() -> WidgetId {
    WidgetId::from_hash("split")
}

/// One frame: a 401×100 row splitter at the origin; the 1 px rule leaves a 400 span, seam center at ratio · 400 + 0.5, grab bar 6 px.
///
/// One snapshot per record pass: action input records twice, signals fire on pass A.
const QUIET: ValueEdges = ValueEdges {
    changed: false,
    committed: false,
};
const CHANGED: ValueEdges = ValueEdges {
    changed: true,
    committed: false,
};

fn frame_with(h: &mut Harness, ratio: &mut f32) -> Passes<ValueEdges> {
    h.frame_passes(|ui| {
        Splitter::row(ratio)
            .id(split_id())
            .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
            .min_pane(50.0)
            .show(ui, |_, _| {})
            .edges()
    })
}

#[test]
fn divider_drag_maps_pointer_to_ratio_without_relayout() {
    let mut h = Harness::mono(SURFACE);
    let mut ratio = 0.5;
    frame_with(&mut h, &mut ratio);
    frame_with(&mut h, &mut ratio);

    // Ratio 0.5: grab bar [197.5, 203.5). Drag 100 px right: pointer 300.5 → first = 300 → 0.75.
    h.press(Vec2::new(200.5, 50.0), PointerButton::Left);
    h.drag_to(Vec2::new(300.5, 50.0));
    let moved = frame_with(&mut h, &mut ratio);
    assert_eq!(
        ratio, 0.75,
        "pointer 300.5 over span 400 → 0.75, got {ratio}"
    );
    // Pass A lays panes out at the pointer; the binding takes the arranged ratio on pass B.
    assert_eq!(*moved.a(), QUIET, "{moved:?}");
    assert_eq!(moved.b(), Some(&CHANGED), "{moved:?}");

    // A later drag movement records once; the caller gets the prior ratio until the next record.
    h.drag_to(Vec2::new(999.0, 50.0));
    let held = frame_with(&mut h, &mut ratio);
    assert_eq!(held.len(), 1);
    assert_eq!(
        ratio, 0.75,
        "model holds the prior arranged ratio for one record, got {ratio}"
    );
    assert_eq!(*held.a(), QUIET, "the binding did not move this frame");
    let rect = h.arranged(split_id().with("first"));
    assert_eq!(
        rect.size.w, 350.0,
        "min_pane(50) stops the current layout at 350 px, got {}",
        rect.size.w
    );

    h.drag_to(Vec2::new(998.0, 50.0));
    let caught_up = frame_with(&mut h, &mut ratio);
    assert_eq!(caught_up.len(), 1);
    assert_eq!(
        ratio, 0.875,
        "the next record writes back the arranged 350/400 ratio, got {ratio}"
    );
    assert_eq!(*caught_up.a(), CHANGED);

    // Release commits the held ratio.
    h.release(PointerButton::Left);
    h.hover(Vec2::new(100.0, 50.0));
    let released = frame_with(&mut h, &mut ratio);
    assert_eq!(ratio, 0.875, "ratio holds after release, got {ratio}");
    assert_eq!(
        *released.a(),
        ValueEdges {
            changed: false,
            committed: true
        },
        "the release commits the held ratio",
    );
    assert_eq!(released.b(), Some(&QUIET));
    let after = frame_with(&mut h, &mut ratio);
    assert_eq!((after.len(), *after.a()), (1, QUIET));

    // Double-click (seam at 350.5) resets to the center: layout takes 0.5 on pass A, the binding commits on pass B.
    let seam = Vec2::new(350.5, 50.0);
    h.press(seam, PointerButton::Left);
    frame_with(&mut h, &mut ratio);
    h.release(PointerButton::Left);
    frame_with(&mut h, &mut ratio);
    h.press(seam, PointerButton::Left);
    frame_with(&mut h, &mut ratio);
    h.release(PointerButton::Left);
    let reset = frame_with(&mut h, &mut ratio);
    assert_eq!(reset.len(), 2, "premise: a double-click records twice");
    assert_eq!(ratio, 0.5, "the reset writes the center, got {ratio}");
    assert_eq!(*reset.a(), QUIET, "{reset:?}");
    assert_eq!(
        reset.b(),
        Some(&ValueEdges {
            changed: true,
            committed: true
        }),
        "{reset:?}"
    );
    let settled = frame_with(&mut h, &mut ratio);
    assert_eq!((settled.len(), *settled.a()), (1, QUIET));
}

#[test]
fn divider_drag_is_scale_invariant() {
    for scale in [0.5, 1.0, 2.0] {
        let mut h = Harness::mono(UVec2::new(1_000, 400));
        let mut ratio = 0.5;
        let frame = |h: &mut Harness, ratio: &mut f32| {
            h.frame(|ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("scaled-split-parent"))
                    .transform(TranslateScale::from_scale(scale))
                    .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
                    .show(ui, |ui| {
                        Splitter::row(ratio)
                            .id(split_id())
                            .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
                            .min_pane(50.0)
                            .show(ui, |_, _| {});
                    });
            });
        };
        frame(&mut h, &mut ratio);
        frame(&mut h, &mut ratio);

        h.press(split_id().with("divider"), PointerButton::Left);
        // Drag targets along the splitter's x, read before the drag.
        let layout = h.arranged(split_id());
        let transform = h.transform(split_id());
        let target = |x: f32| transform.apply_point(layout.min + Vec2::new(x, 50.0));
        h.hover(target(300.5));
        frame(&mut h, &mut ratio);

        assert_eq!(
            ratio, 0.75,
            "logical pointer 300.5 over span 400 at {scale}× produced {ratio}"
        );

        h.hover(target(380.0));
        frame(&mut h, &mut ratio);
        assert_eq!(
            h.arranged(split_id().with("first")).size.w,
            350.0,
            "50 px second-pane minimum at {scale}×",
        );

        h.hover(target(381.0));
        frame(&mut h, &mut ratio);
        assert_eq!(
            ratio, 0.875,
            "minimum-pane ratio at {scale}× produced {ratio}"
        );
    }
}

#[test]
fn divider_and_pane_stop_together_when_content_is_rigid() {
    for (row, rigid_half, expected_ratio) in [
        (true, SplitHalf::First, 0.45),
        (true, SplitHalf::Second, 0.55),
        (false, SplitHalf::First, 0.45),
        (false, SplitHalf::Second, 0.55),
    ] {
        let mut h = Harness::mono(SURFACE);
        let mut ratio = 0.5;
        let frame = |h: &mut Harness, ratio: &mut f32| {
            h.frame(|ui| {
                let splitter = if row {
                    Splitter::row(ratio)
                } else {
                    Splitter::col(ratio)
                };
                splitter
                    .id(split_id())
                    .size(if row {
                        (Sizing::fixed(401.0), Sizing::fixed(100.0))
                    } else {
                        (Sizing::fixed(100.0), Sizing::fixed(401.0))
                    })
                    .min_pane(50.0)
                    .show(ui, |ui, half| {
                        if half == rigid_half {
                            Block::new()
                                .id(split_id().with("rigid"))
                                .size(if row {
                                    (Sizing::fixed(180.0), Sizing::FILL)
                                } else {
                                    (Sizing::FILL, Sizing::fixed(180.0))
                                })
                                .show(ui);
                        }
                    });
            })
            .processing
        };
        frame(&mut h, &mut ratio);
        frame(&mut h, &mut ratio);

        h.press(
            if row {
                Vec2::new(200.5, 50.0)
            } else {
                Vec2::new(50.0, 200.5)
            },
            PointerButton::Left,
        );
        let activation_main = 210.5;
        h.hover(if row {
            Vec2::new(activation_main, 50.0)
        } else {
            Vec2::new(50.0, activation_main)
        });
        frame(&mut h, &mut ratio);

        let pointer_main = if rigid_half == SplitHalf::First {
            -100.0
        } else {
            500.0
        };
        h.hover(if row {
            Vec2::new(pointer_main, 50.0)
        } else {
            Vec2::new(50.0, pointer_main)
        });
        assert_eq!(
            frame(&mut h, &mut ratio),
            FrameProcessing::SingleLayout,
            "active drag movement must not request a second layout"
        );

        assert_eq!(
            ratio, 0.525,
            "model keeps the prior arranged ratio for one record"
        );
        let shrinking = h.node_for_widget_id(split_id().with(match rigid_half {
            SplitHalf::First => "first",
            SplitHalf::Second => "second",
        }));
        let shrinking_rect = h.ui().arranged_rect(Layer::Main, shrinking);
        assert_eq!(
            if row {
                shrinking_rect.size.w
            } else {
                shrinking_rect.size.h
            },
            180.0,
            "{rigid_half:?} pane stops at its rigid content floor"
        );
        let rigid_rect = h.arranged(split_id().with("rigid"));
        assert_eq!(
            if row {
                rigid_rect.size.w
            } else {
                rigid_rect.size.h
            },
            180.0,
            "rigid content remains laid out"
        );
        let first_rect = h.arranged(split_id().with("first"));
        let divider_rect = h
            .rect(split_id().with("divider"))
            .expect("divider arranged");
        let divider_center = if row {
            divider_rect.center().x
        } else {
            divider_rect.center().y
        };
        let first_edge = if row {
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
        h.hover(if row {
            Vec2::new(next_pointer, 50.0)
        } else {
            Vec2::new(50.0, next_pointer)
        });
        assert_eq!(frame(&mut h, &mut ratio), FrameProcessing::SingleLayout);
        assert_eq!(
            ratio, expected_ratio,
            "{rigid_half:?} next record writes back its content floor"
        );
    }
}

#[test]
fn divider_requests_the_resize_cursor() {
    let mut h = Harness::mono(SURFACE);
    let mut ratio = 0.5;
    frame_with(&mut h, &mut ratio);
    frame_with(&mut h, &mut ratio);
    assert_eq!(
        h.ui().window_requests().levels.cursor,
        CursorIcon::Default,
        "idle frame keeps the arrow"
    );

    // Hovering the grab bar requests the horizontal-resize cursor.
    h.hover(split_id().with("divider"));
    frame_with(&mut h, &mut ratio);
    assert_eq!(
        h.ui().window_requests().levels.cursor,
        CursorIcon::EwResize,
        "hover shows resize"
    );

    // Mid-drag the cursor holds after the pointer leaves the bar.
    h.press(split_id().with("divider"), PointerButton::Left);
    h.drag_to(Vec2::new(320.0, 50.0));
    frame_with(&mut h, &mut ratio);
    assert_eq!(
        h.ui().window_requests().levels.cursor,
        CursorIcon::EwResize,
        "drag holds resize off-bar"
    );

    // Release over a pane resets the cursor to the arrow.
    h.release(PointerButton::Left);
    h.hover(Vec2::new(50.0, 50.0));
    frame_with(&mut h, &mut ratio);
    assert_eq!(
        h.ui().window_requests().levels.cursor,
        CursorIcon::Default,
        "leave resets to the arrow"
    );

    // A vertical splitter's divider asks for the other axis.
    let mut h = Harness::mono(SURFACE);
    let mut ratio = 0.5;
    let frame = |h: &mut Harness, ratio: &mut f32| {
        h.frame(|ui| {
            Splitter::col(ratio)
                .id(split_id())
                .size((Sizing::fixed(100.0), Sizing::fixed(201.0)))
                .show(ui, |_, _| {});
        });
    };
    frame(&mut h, &mut ratio);
    frame(&mut h, &mut ratio);
    // Free span 200 at ratio 0.5 → grab bar rows [97.5, 103.5).
    h.hover(split_id().with("divider"));
    frame(&mut h, &mut ratio);
    assert_eq!(
        h.ui().window_requests().levels.cursor,
        CursorIcon::NsResize,
        "column split resizes vertically"
    );

    for (grab_thickness, rule_thickness) in [(3.0, 9.0), (6.0, 0.0)] {
        let mut h = Harness::mono(SURFACE);
        let mut ratio = 0.5;
        let frame = |h: &mut Harness, ratio: &mut f32| {
            h.frame(|ui| {
                let style = SplitterTheme {
                    grab_thickness,
                    rule_thickness,
                    ..SplitterTheme::default()
                };
                Splitter::row(ratio)
                    .id(split_id())
                    .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
                    .style(&style)
                    .show(ui, |_, _| {});
            });
        };
        frame(&mut h, &mut ratio);
        frame(&mut h, &mut ratio);

        let first_rect = h.arranged(split_id().with("first"));
        let divider_rect = h
            .rect(split_id().with("divider"))
            .expect("divider arranged");
        assert_eq!(divider_rect.size.w, grab_thickness);
        assert_eq!(
            divider_rect.center().x,
            first_rect.max().x + rule_thickness * 0.5,
            "grab bar stays centered whether narrower or wider than the rule"
        );
    }
}

/// A splitter disabled mid-drag lets go at once: the divider drops its active fill and the
/// resize cursor, the pointer stops steering, the panes hold the last arranged 0.75, and the
/// release commits nothing.
#[test]
fn a_splitter_disabled_mid_drag_lets_go() {
    let frame = |h: &mut Harness, ratio: &mut f32, disabled: bool| {
        h.frame_passes(|ui| {
            Splitter::row(ratio)
                .id(split_id())
                .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
                .disabled(disabled)
                .show(ui, |_, _| {})
                .edges()
        })
    };
    let divider = split_id().with("divider");
    let fill = |h: &Harness| {
        let node = h.node_of(divider).expect("the divider records").node;
        h.ui_ref().tree(Layer::Main).chrome(node).is_some()
    };
    let mut h = Harness::mono(SURFACE);
    let mut ratio = 0.5;
    frame(&mut h, &mut ratio, false);
    frame(&mut h, &mut ratio, false);
    h.press(Vec2::new(200.5, 50.0), PointerButton::Left);
    h.drag_to(Vec2::new(300.5, 50.0));
    frame(&mut h, &mut ratio, false);
    assert_eq!(ratio, 0.75);
    assert!(fill(&h), "premise: the live drag paints the divider");

    h.drag_to(Vec2::new(350.5, 50.0));
    let disabled = frame(&mut h, &mut ratio, true);
    assert!(!fill(&h), "a disabled divider paints no drag fill");
    assert_eq!(h.ui().window_requests().levels.cursor, CursorIcon::Default);
    assert_eq!(ratio, 0.75, "the pointer no longer steers");
    assert_eq!(h.arranged(split_id().with("first")).size.w, 300.0);
    assert_eq!(disabled.count_where(|e| *e != QUIET), 0, "{disabled:?}");

    h.release(PointerButton::Left);
    let released = frame(&mut h, &mut ratio, true);
    assert_eq!(
        released.count_where(|e| *e != QUIET),
        0,
        "no commit: {released:?}"
    );
    assert_eq!(ratio, 0.75);
}

#[test]
fn ratio_at_maps_center_edges_and_floors() {
    // extent 406, reserved 6 → span 400; pointer 203 → first = 200 → ratio 0.5.
    let cases = [
        (203.0, 406.0, 6.0, 0.0, 0.0, 0.5),
        (3.0, 406.0, 6.0, 0.0, 0.0, 0.0),   // at the left stop
        (403.0, 406.0, 6.0, 0.0, 0.0, 1.0), // at the right stop
        (-50.0, 406.0, 6.0, 0.0, 0.0, 0.0), // past the ends clamps
        (999.0, 406.0, 6.0, 0.0, 0.0, 1.0),
        (103.0, 406.0, 6.0, 0.0, 0.0, 0.25),   // quarter point
        (10.0, 406.0, 6.0, 50.0, 0.0, 0.125),  // min_pane floors first: 50/400
        (395.0, 406.0, 6.0, 50.0, 0.0, 0.875), // …and second: 350/400
        (10.0, 406.0, 6.0, 0.0, 0.1, 0.1),     // min_ratio floors first
        (395.0, 406.0, 6.0, 0.0, 0.1, 0.9),    // …and second: 1 − 0.1
        (10.0, 406.0, 6.0, 50.0, 0.1, 0.125),  // the larger floor wins: 50/400 > 0.1
        (10.0, 406.0, 6.0, 20.0, 0.1, 0.1),    // …and here 0.1 > 20/400
        (7.0, 406.0, 6.0, 300.0, 0.0, 0.5),    // floors can't both fit → center
        (10.0, 4.0, 6.0, 0.0, 0.0, 0.5),       // degenerate extent
    ];
    for (pos, extent, thickness, min_pane, min_ratio, want) in cases {
        let seam = Seam {
            extent,
            reserved: thickness,
            min_pane,
            min_ratio,
        };
        let got = seam.ratio_at(pos);
        assert_eq!(
            got, want,
            "p2r({pos},{extent},{thickness},{min_pane},{min_ratio})={got} want {want}"
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
        let mut h = Harness::mono(SURFACE);
        let mut ratio = ratio;
        h.frame(|ui| {
            Splitter::row(&mut ratio)
                .id(split_id())
                .size((Sizing::fixed(401.0), Sizing::fixed(100.0)))
                .show(ui, |_, _| {});
        });
        let first = h.node_for_widget_id(split_id().with("first"));
        let second = h.node_for_widget_id(split_id().with("second"));
        let rects = &h.ui_ref().layout(Layer::Main).rect;
        assert_eq!(
            [rects[first.idx()].size.w, rects[second.idx()].size.w],
            expected,
            "ratio {ratio}",
        );
    }
}

/// A focused divider takes the window splitter's keys through the drag's mapping, held by `min_pane`. Seam at `400 r + 0.5`: an arrow moves `r` by 10 / 400 = 0.025, and with Shift ten of those, 0.25; End holds at 1 − 50 / 400 = 0.875; Home at 0.125. Each key commits once.
#[test]
fn a_focused_divider_moves_by_the_keys_within_the_floors() {
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::modifiers::Modifiers;
    use crate::primitives::math::domain;

    let mut h = Harness::mono(SURFACE);
    let mut ratio = 0.5;
    frame_with(&mut h, &mut ratio);
    frame_with(&mut h, &mut ratio);
    h.set_focus(split_id().with("divider"));
    frame_with(&mut h, &mut ratio);
    for (mods, key, want) in [
        (Modifiers::NONE, Key::ArrowRight, 0.525),
        (Modifiers::NONE, Key::ArrowLeft, 0.5),
        (Modifiers::NONE, Key::ArrowDown, 0.5),
        (Modifiers::NONE, Key::End, 0.875),
        (Modifiers::NONE, Key::Home, 0.125),
        (Modifiers::SHIFT, Key::ArrowRight, 0.375),
    ] {
        h.set_modifiers(mods);
        h.key(key);
        let first = frame_with(&mut h, &mut ratio);
        let second = frame_with(&mut h, &mut ratio);
        assert!(domain::approx_eq(ratio, want), "{key:?}: {ratio} != {want}");
        let commits = [&first, &second]
            .iter()
            .map(|passes| passes.count_where(|edges| edges.committed))
            .sum::<usize>();
        let moved = key != Key::ArrowDown;
        assert_eq!(commits, usize::from(moved), "{key:?}: commits");
    }
}
