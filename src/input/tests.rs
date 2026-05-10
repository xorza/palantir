use crate::Ui;
use crate::forest::element::Configure;
use crate::input::sense::Sense;
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::support::testing::{begin, click_at, press_at, release_left, ui_at};
use crate::widgets::{button::Button, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn input_state_press_release_emits_click() {
    // Drive the input state machine without any windowing toolkit. Two frames:
    // frame 1 lays out a button so its rect lands in `last_rects`; then a
    // press+release pair over its rect produces `clicked = true` on frame 2.
    let mut ui = Ui::new();

    // Frame 1: build, layout, end_frame to populate last_rects.
    begin(&mut ui, UVec2::new(200, 80));
    let _root = Panel::hstack()
        .auto_id()
        .show(&mut ui, |ui| {
            Button::new()
                .id_salt("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Press inside the button, release inside.
    click_at(&mut ui, Vec2::new(50.0, 20.0));

    // Frame 2: rebuild; widgets should observe the click in build_ui.
    begin(&mut ui, UVec2::new(200, 80));
    let mut got_click = false;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        let r = Button::new()
            .id_salt("target")
            .label("hi")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
        got_click = r.clicked();
    });
    assert!(got_click, "press+release inside button rect should click");

    // Click does not stick: next frame without input must clear it.
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    begin(&mut ui, UVec2::new(200, 80));
    let mut still_clicking = false;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        still_clicking = Button::new()
            .id_salt("target")
            .label("hi")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui)
            .clicked();
    });
    assert!(!still_clicking, "click is one-shot");
}

#[test]
fn stack_with_sense_none_passes_clicks_through() {
    // HStack default Sense::NONE — clicking on its background (between children)
    // doesn't fire `clicked` on the stack. Clicking on a child still fires on the child.
    let mut ui = ui_at(UVec2::new(200, 100));
    let _stack_node = Panel::hstack()
        .auto_id()
        .padding(20.0) // creates "background" area to click
        .show(&mut ui, |ui| {
            Button::new()
                .id_salt("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Press inside the HStack's padding (not over any child).
    click_at(&mut ui, Vec2::new(5.0, 5.0));

    ui.begin_frame(Display::default());
    let mut child_clicked = false;
    let stack_resp = Panel::hstack().auto_id().padding(20.0).show(&mut ui, |ui| {
        child_clicked = Button::new()
            .id_salt("inside")
            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
            .show(ui)
            .clicked();
    });
    assert!(
        !stack_resp.clicked(),
        "non-sensing stack does not capture clicks"
    );
    assert!(
        !child_clicked,
        "click on stack background doesn't reach child"
    );
}

#[test]
fn stack_with_sense_click_captures_clicks() {
    // Opt-in: HStack::sense(Sense::CLICK) makes the container clickable.
    // Use `with_id` so the stack has the same WidgetId on both frames; otherwise
    // `auto_stable` would give different ids (different call sites in the test).
    let mut ui = ui_at(UVec2::new(200, 100));
    let _stack_node = Panel::hstack()
        .id_salt("clickable_card")
        .padding(20.0)
        .sense(Sense::CLICK)
        .show(&mut ui, |ui| {
            Button::new()
                .id_salt("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    click_at(&mut ui, Vec2::new(5.0, 5.0));

    ui.begin_frame(Display::default());
    let stack_resp = Panel::hstack()
        .id_salt("clickable_card")
        .padding(20.0)
        .sense(Sense::CLICK)
        .show(&mut ui, |ui| {
            Button::new()
                .id_salt("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    assert!(
        stack_resp.clicked(),
        "stack with Sense::CLICK fires on background click"
    );
}

#[test]
fn stack_with_sense_hover_reports_hover_but_passes_clicks_through() {
    // Sense::HOVER: visible to hover state but transparent to click capture.
    // Useful for tooltips, cursor changes, row highlights.
    let mut ui = ui_at(UVec2::new(200, 100));
    let _stack_node = Panel::hstack()
        .id_salt("hover_only")
        .padding(20.0)
        .sense(Sense::HOVER)
        .show(&mut ui, |ui| {
            Button::new()
                .id_salt("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Move pointer over stack's padding area (not over the button).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(5.0, 5.0)));

    // Press + release on the same spot.
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    release_left(&mut ui);

    ui.begin_frame(Display::default());
    let mut child_clicked = false;
    let stack_resp = Panel::hstack()
        .id_salt("hover_only")
        .padding(20.0)
        .sense(Sense::HOVER)
        .show(&mut ui, |ui| {
            child_clicked = Button::new()
                .id_salt("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui)
                .clicked();
        });

    assert!(
        stack_resp.hovered(),
        "Sense::HOVER stack reports hovered=true"
    );
    assert!(
        !stack_resp.clicked(),
        "Sense::HOVER does not capture clicks"
    );
    assert!(
        !child_clicked,
        "no clickable widget under cursor → no click anywhere"
    );
}

#[test]
fn input_state_release_outside_does_not_click() {
    let mut ui = ui_at(UVec2::new(400, 80));
    let _root = Panel::hstack()
        .auto_id()
        .show(&mut ui, |ui| {
            Button::new()
                .id_salt("target")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    press_at(&mut ui, Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 20.0))); // outside
    release_left(&mut ui);

    ui.begin_frame(Display::default());
    let mut got_click = false;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        got_click = Button::new()
            .id_salt("target")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui)
            .clicked();
    });
    assert!(
        !got_click,
        "release outside the original widget cancels click"
    );
}

#[test]
fn click_on_overflow_outside_clipped_parent_is_suppressed() {
    // A clip=true panel at (0,0,100,100) wraps a 200x200 button. The button
    // visually extends past the panel's rect; clicks on the overflow should
    // miss the button (input must respect the clip cascade).
    let mut ui = Ui::new();

    // Frame 1: build + layout so last_rects gets populated.
    begin(&mut ui, UVec2::new(400, 400));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("clipper")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .clip_rect()
            .show(ui, |ui| {
                Button::new()
                    .id_salt("inner")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui);
            });
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Click well outside the panel's 100x100 clip rect but inside the button's
    // raw 200x200 rect (overflow region). With clip-aware hit-test this misses.
    click_at(&mut ui, Vec2::new(150.0, 150.0));

    // Frame 2: read .clicked() from the button's response.
    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("clipper")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .clip_rect()
            .show(ui, |ui| {
                clicked = Button::new()
                    .id_salt("inner")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui)
                    .clicked();
            });
    });
    assert!(
        !clicked,
        "click on overflow outside clip should not register"
    );
}

#[test]
fn zoom_panel_routes_clicks_to_world_rendered_button() {
    use crate::primitives::transform::TranslateScale;

    // ZStack with transform=scale(2) wrapping a 50x50 button. The button's
    // logical rect is (0,0,50,50) but its world (rendered) rect is
    // (0,0,100,100). A click at logical (5,5) must hit; a click at (75,75)
    // (inside world bounds, outside logical bounds) must also hit.
    let mut ui = ui_at(UVec2::new(400, 400));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(2.0))
            .show(ui, |ui| {
                Button::new()
                    .id_salt("inner")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui);
            });
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Click at world (75, 75) — inside the zoomed 100x100 bounds.
    click_at(&mut ui, Vec2::new(75.0, 75.0));

    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(2.0))
            .show(ui, |ui| {
                clicked = Button::new()
                    .id_salt("inner")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui)
                    .clicked();
            });
    });
    assert!(
        clicked,
        "click inside world-rendered (zoomed) bounds should hit"
    );
}

#[test]
fn click_outside_zoomed_bounds_does_not_hit() {
    use crate::primitives::transform::TranslateScale;

    let mut ui = ui_at(UVec2::new(400, 400));
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(0.5))
            .show(ui, |ui| {
                Button::new()
                    .id_salt("inner")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui);
            });
    });
    ui.end_frame_record_phase();
    ui.end_frame_paint_phase();
    // Button's world rect under scale=0.5 is 25x25. Click at (40, 40) is
    // inside the LOGICAL rect but outside the world-rendered rect.
    click_at(&mut ui, Vec2::new(40.0, 40.0));

    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().auto_id().show(&mut ui, |ui| {
        Panel::zstack()
            .id_salt("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(0.5))
            .show(ui, |ui| {
                clicked = Button::new()
                    .id_salt("inner")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui)
                    .clicked();
            });
    });
    assert!(!clicked, "click outside world-rendered bounds should miss");
}

mod drag {
    use crate::Ui;
    use crate::forest::element::Configure;
    use crate::forest::widget_id::WidgetId;
    use crate::input::sense::Sense;
    use crate::input::{InputEvent, PointerButton};
    use crate::layout::types::display::Display;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::ui_at;
    use crate::widgets::panel::Panel;
    use glam::{UVec2, Vec2};

    fn build_clickable(ui: &mut Ui) {
        Panel::hstack()
            .id_salt("target")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .sense(Sense::CLICK)
            .show(ui, |_| {});
    }

    fn id() -> WidgetId {
        WidgetId::from_hash("target")
    }

    #[test]
    fn drag_delta_none_before_press() {
        let mut ui = ui_at(UVec2::new(200, 200));
        build_clickable(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        assert_eq!(ui.input.drag_delta(id()), None, "no press → no drag");
    }

    #[test]
    fn drag_delta_tracks_pointer_minus_press() {
        let mut ui = ui_at(UVec2::new(200, 200));
        build_clickable(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 30.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerMoved(Vec2::new(80.0, 70.0)));

        assert_eq!(
            ui.input.drag_delta(id()),
            Some(Vec2::new(60.0, 40.0)),
            "delta = current - press_pos"
        );
    }

    #[test]
    fn drag_delta_persists_when_pointer_leaves_widget_rect() {
        // The whole point of "rect-independent": once captured, the
        // pointer can wander outside the widget and the delta keeps
        // tracking. Pin so a future tightening doesn't gate drag on
        // staying inside the originating rect.
        let mut ui = ui_at(UVec2::new(400, 400));
        build_clickable(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        // Move well outside the 100x100 widget rect.
        ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 200.0)));

        assert_eq!(ui.input.drag_delta(id()), Some(Vec2::new(250.0, 150.0)));
    }

    #[test]
    fn drag_delta_clears_on_release() {
        let mut ui = ui_at(UVec2::new(200, 200));
        build_clickable(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.on_input(InputEvent::PointerMoved(Vec2::new(30.0, 30.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerMoved(Vec2::new(70.0, 70.0)));
        assert!(ui.input.drag_delta(id()).is_some());

        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
        assert_eq!(
            ui.input.drag_delta(id()),
            None,
            "release ends the drag (active cleared)"
        );
    }

    #[test]
    fn drag_delta_none_when_pointer_left_surface() {
        let mut ui = ui_at(UVec2::new(200, 200));
        build_clickable(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.on_input(InputEvent::PointerMoved(Vec2::new(40.0, 40.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerLeft);

        // press_pos kept; pointer.pos cleared → no current delta
        // available. (A polish pass could freeze last_pos so drags can
        // continue past the surface edge — defer.)
        assert_eq!(ui.input.drag_delta(id()), None);
    }

    #[test]
    fn drag_delta_only_for_active_widget() {
        let mut ui = ui_at(UVec2::new(200, 200));
        build_clickable(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.on_input(InputEvent::PointerMoved(Vec2::new(20.0, 20.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerMoved(Vec2::new(60.0, 50.0)));

        let other = WidgetId::from_hash("other");
        assert_eq!(
            ui.input.drag_delta(other),
            None,
            "only the captured widget sees the drag delta"
        );
    }

    #[test]
    fn drag_delta_none_when_press_missed_all_widgets() {
        // Press over empty space ⇒ no widget captures, press_pos stays
        // None. A subsequent move doesn't synthesize a drag. Wrap the
        // small clickable in an outer non-clickable panel so the root
        // doesn't auto-fill the surface and swallow the press.
        let surface = UVec2::new(400, 400);
        let build = |ui: &mut Ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::hstack()
                    .id_salt("target")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .sense(Sense::CLICK)
                    .show(ui, |_| {});
            });
        };
        let mut ui = ui_at(surface);
        build(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.begin_frame(Display::from_physical(surface, 1.0));
        build(&mut ui);
        ui.on_input(InputEvent::PointerMoved(Vec2::new(200.0, 200.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerMoved(Vec2::new(250.0, 220.0)));
        assert_eq!(ui.input.drag_delta(id()), None);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
    }
}

mod scroll {
    use crate::input::{InputEvent, InputState};
    use crate::ui::cascade::CascadeResult;
    use glam::Vec2;
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, MouseScrollDelta, TouchPhase, WindowEvent};

    fn wheel(delta: MouseScrollDelta) -> WindowEvent {
        WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta,
            phase: TouchPhase::Moved,
        }
    }

    #[test]
    fn from_winit_line_delta_scales_by_step_pixels_and_flips_both_axes() {
        // winit's +y wheel = rotation away from user = scroll up; +x wheel
        // = swipe right (reveal content right = pan offset forward). We flip
        // both so palantir's +delta means "advance the scroll offset."
        let ev = InputEvent::from_winit(&wheel(MouseScrollDelta::LineDelta(2.0, 1.0)), 1.0)
            .expect("wheel produces a Scroll event");
        match ev {
            InputEvent::Scroll(d) => {
                assert_eq!(d.x, -80.0, "2 lines right → -2·SCROLL_LINE_PIXELS");
                assert_eq!(d.y, -40.0, "1 line up → -SCROLL_LINE_PIXELS");
            }
            _ => panic!("expected Scroll, got {ev:?}"),
        }
    }

    #[test]
    fn from_winit_pixel_delta_divides_by_scale_factor_and_flips_both_axes() {
        let ev = InputEvent::from_winit(
            &wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                60.0, -120.0,
            ))),
            2.0,
        )
        .expect("pixel-delta wheel produces a Scroll event");
        match ev {
            InputEvent::Scroll(d) => {
                // x: -(60 / 2) = -30. y: -(-120 / 2) = 60.
                assert_eq!(d, Vec2::new(-30.0, 60.0));
            }
            _ => panic!("expected Scroll, got {ev:?}"),
        }
    }

    #[test]
    fn on_input_accumulates_scroll_delta() {
        let mut state = InputState::new();
        let cascades = CascadeResult::default();
        state.on_input(InputEvent::Scroll(Vec2::new(0.0, 40.0)), &cascades);
        state.on_input(InputEvent::Scroll(Vec2::new(5.0, -10.0)), &cascades);
        assert_eq!(state.frame_scroll_delta, Vec2::new(5.0, 30.0));
    }

    #[test]
    fn end_frame_clears_scroll_delta() {
        let mut state = InputState::new();
        let cascades = CascadeResult::default();
        state.on_input(InputEvent::Scroll(Vec2::new(7.0, 7.0)), &cascades);
        assert_eq!(state.frame_scroll_delta, Vec2::new(7.0, 7.0));
        state.end_frame(&cascades);
        assert_eq!(state.frame_scroll_delta, Vec2::ZERO);
    }
}

mod scroll_routing {
    use crate::forest::element::Configure;
    use crate::input::InputEvent;
    use crate::input::sense::Sense;
    use crate::layout::types::sizing::Sizing;
    use crate::support::testing::ui_at;
    use crate::widgets::panel::Panel;
    use glam::{UVec2, Vec2};

    #[test]
    fn nested_scroll_panels_route_to_innermost_under_pointer() {
        // Outer scroll panel (200×200) wraps an inner scroll panel (100×100)
        // at the top-left. Pointer over the inner area must claim the
        // delta on the inner; pointer outside inner but inside outer
        // routes to outer.
        let mut ui = ui_at(UVec2::new(300, 300));
        Panel::zstack()
            .id_salt("outer")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(&mut ui, |ui| {
                Panel::zstack()
                    .id_salt("inner")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                    .sense(Sense::SCROLL)
                    .show(ui, |_| {});
            });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.begin_frame(crate::layout::types::display::Display::default());
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 5.0)));
        Panel::zstack()
            .id_salt("outer")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(&mut ui, |ui| {
                Panel::zstack()
                    .id_salt("inner")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                    .sense(Sense::SCROLL)
                    .show(ui, |_| {});
            });
        let inner_id = crate::forest::widget_id::WidgetId::from_hash("inner");
        let outer_id = crate::forest::widget_id::WidgetId::from_hash("outer");
        assert_eq!(ui.input.scroll_delta_for(inner_id), Vec2::new(0.0, 5.0));
        assert_eq!(ui.input.scroll_delta_for(outer_id), Vec2::ZERO);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
    }

    #[test]
    fn scroll_delta_zero_for_non_target() {
        let mut ui = ui_at(UVec2::new(200, 200));
        Panel::zstack()
            .id_salt("scroller")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(&mut ui, |_| {});
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.begin_frame(crate::layout::types::display::Display::default());
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 9.0)));
        Panel::zstack()
            .id_salt("scroller")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(&mut ui, |_| {});
        let unrelated = crate::forest::widget_id::WidgetId::from_hash("nope");
        assert_eq!(ui.input.scroll_delta_for(unrelated), Vec2::ZERO);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
    }

    #[test]
    fn pointer_left_clears_scroll_target() {
        let mut ui = ui_at(UVec2::new(200, 200));
        Panel::zstack()
            .id_salt("scroller")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(&mut ui, |_| {});
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.begin_frame(crate::layout::types::display::Display::default());
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 50.0)));
        ui.on_input(InputEvent::PointerLeft);
        ui.on_input(InputEvent::Scroll(Vec2::new(0.0, 5.0)));
        Panel::zstack()
            .id_salt("scroller")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
            .sense(Sense::SCROLL)
            .show(&mut ui, |_| {});
        let id = crate::forest::widget_id::WidgetId::from_hash("scroller");
        assert_eq!(
            ui.input.scroll_delta_for(id),
            Vec2::ZERO,
            "PointerLeft drops scroll target so the delta is unclaimed",
        );
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
    }
}

mod keyboard {
    use crate::input::keyboard::{Key, Modifiers, TextChunk, key_from_winit};
    use crate::input::{InputEvent, InputState};
    use crate::ui::cascade::CascadeResult;
    use winit::event::WindowEvent;
    use winit::keyboard::{Key as WK, NamedKey};

    // `winit::event::KeyEvent` carries a platform_specific field that's
    // non-portable to construct in tests, so we exercise the translation
    // helper `key_from_winit` directly. The KeyboardInput→KeyDown
    // wrapping in `from_winit` is a one-line `match event.state` — small
    // enough that integration coverage of it can wait for a manual
    // smoke-test in the showcase.

    #[test]
    fn key_from_winit_named_arrows() {
        assert_eq!(
            key_from_winit(&WK::Named(NamedKey::ArrowLeft)),
            Key::ArrowLeft
        );
        assert_eq!(
            key_from_winit(&WK::Named(NamedKey::ArrowRight)),
            Key::ArrowRight
        );
        assert_eq!(key_from_winit(&WK::Named(NamedKey::ArrowUp)), Key::ArrowUp);
        assert_eq!(
            key_from_winit(&WK::Named(NamedKey::ArrowDown)),
            Key::ArrowDown
        );
    }

    #[test]
    fn key_from_winit_editing_keys() {
        assert_eq!(
            key_from_winit(&WK::Named(NamedKey::Backspace)),
            Key::Backspace
        );
        assert_eq!(key_from_winit(&WK::Named(NamedKey::Delete)), Key::Delete);
        assert_eq!(key_from_winit(&WK::Named(NamedKey::Home)), Key::Home);
        assert_eq!(key_from_winit(&WK::Named(NamedKey::End)), Key::End);
        assert_eq!(key_from_winit(&WK::Named(NamedKey::Enter)), Key::Enter);
        assert_eq!(key_from_winit(&WK::Named(NamedKey::Escape)), Key::Escape);
    }

    #[test]
    fn key_from_winit_character_carries_first_char() {
        // Shift+'a' arrives as Character("A") post-layout — should keep
        // the capitalized form.
        assert_eq!(key_from_winit(&WK::Character("A".into())), Key::Char('A'));
        assert_eq!(key_from_winit(&WK::Character("é".into())), Key::Char('é'));
    }

    #[test]
    fn key_from_winit_unknown_key_falls_back_to_other() {
        // `F24` exists in NamedKey but isn't enumerated in our `Key` —
        // should land in the catch-all rather than dropping the event.
        assert_eq!(key_from_winit(&WK::Named(NamedKey::F24)), Key::Other);
    }

    #[test]
    fn key_from_winit_paging_navigation_keys() {
        assert_eq!(key_from_winit(&WK::Named(NamedKey::PageUp)), Key::PageUp);
        assert_eq!(
            key_from_winit(&WK::Named(NamedKey::PageDown)),
            Key::PageDown
        );
        assert_eq!(key_from_winit(&WK::Named(NamedKey::Tab)), Key::Tab);
        // Space collapses to `Char(' ')` so the editor treats it as
        // ordinary text input — no dedicated variant.
        assert_eq!(key_from_winit(&WK::Named(NamedKey::Space)), Key::Char(' '));
    }

    #[test]
    fn modifiers_from_winit_translates_each_bit() {
        use crate::input::keyboard::modifiers_from_winit;
        use winit::keyboard::ModifiersState;

        // Every flag off → all-default Modifiers.
        let m = modifiers_from_winit(&ModifiersState::empty());
        assert_eq!(m, Modifiers::NONE);

        // Each individual bit maps to the matching field.
        let m = modifiers_from_winit(&ModifiersState::SHIFT);
        assert!(m.shift && !m.ctrl && !m.alt && !m.meta);
        let m = modifiers_from_winit(&ModifiersState::CONTROL);
        assert!(!m.shift && m.ctrl && !m.alt && !m.meta);
        let m = modifiers_from_winit(&ModifiersState::ALT);
        assert!(!m.shift && !m.ctrl && m.alt && !m.meta);
        let m = modifiers_from_winit(&ModifiersState::SUPER);
        assert!(!m.shift && !m.ctrl && !m.alt && m.meta);

        // Combined: shift+meta should set both.
        let m = modifiers_from_winit(&(ModifiersState::SHIFT | ModifiersState::SUPER));
        assert!(m.shift && m.meta && !m.ctrl && !m.alt);
    }

    #[test]
    fn from_winit_ime_commit_emits_text_event() {
        let ev = InputEvent::from_winit(
            &WindowEvent::Ime(winit::event::Ime::Commit("é".into())),
            1.0,
        )
        .expect("Ime::Commit produces a Text event");
        match ev {
            InputEvent::Text(chunk) => assert_eq!(chunk.as_str(), "é"),
            _ => panic!("expected Text, got {ev:?}"),
        }
    }

    #[test]
    fn from_winit_ime_commit_too_long_drops_event() {
        let long = "0123456789abcdef"; // 16 bytes — over inline cap
        let ev = InputEvent::from_winit(
            &WindowEvent::Ime(winit::event::Ime::Commit(long.into())),
            1.0,
        );
        assert!(
            ev.is_none(),
            "oversized IME commit drops cleanly rather than truncating"
        );
    }

    #[test]
    fn keyboard_events_do_not_perturb_scroll_state() {
        // Pin: keyboard plumbing is independent of pointer/scroll. Scroll
        // delta accumulator must stay untouched even as keys, text, and
        // modifier changes flow in.
        let mut state = InputState::new();
        let cascades = CascadeResult::default();
        let before_scroll = state.frame_scroll_delta;
        state.on_input(
            InputEvent::KeyDown {
                key: Key::ArrowLeft,
                repeat: false,
            },
            &cascades,
        );
        state.on_input(InputEvent::Text(TextChunk::new("a").unwrap()), &cascades);
        state.on_input(InputEvent::ModifiersChanged(Modifiers::NONE), &cascades);
        assert_eq!(state.frame_scroll_delta, before_scroll);
    }

    #[test]
    fn keydown_pushes_onto_frame_keys_with_current_modifiers() {
        // Modifiers are captured at push time, not drain time, so a
        // ModifiersChanged event that lands between two KeyDowns
        // attributes correctly.
        let mut state = InputState::new();
        let cascades = CascadeResult::default();

        state.on_input(
            InputEvent::ModifiersChanged(Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }),
            &cascades,
        );
        state.on_input(
            InputEvent::KeyDown {
                key: Key::Char('a'),
                repeat: false,
            },
            &cascades,
        );
        state.on_input(InputEvent::ModifiersChanged(Modifiers::NONE), &cascades);
        state.on_input(
            InputEvent::KeyDown {
                key: Key::Char('b'),
                repeat: true,
            },
            &cascades,
        );

        assert_eq!(state.frame_keys.len(), 2);
        assert_eq!(state.frame_keys[0].key, Key::Char('a'));
        assert!(state.frame_keys[0].mods.ctrl);
        assert!(!state.frame_keys[0].repeat);
        assert_eq!(state.frame_keys[1].key, Key::Char('b'));
        assert!(!state.frame_keys[1].mods.ctrl);
        assert!(state.frame_keys[1].repeat);
    }

    #[test]
    fn text_events_concatenate_into_frame_text() {
        let mut state = InputState::new();
        let cascades = CascadeResult::default();
        state.on_input(InputEvent::Text(TextChunk::new("hé").unwrap()), &cascades);
        state.on_input(InputEvent::Text(TextChunk::new("llo").unwrap()), &cascades);
        assert_eq!(state.frame_text, "héllo");
    }

    #[test]
    fn focus_lands_on_press_over_focusable_widget_and_preserve_holds_it() {
        // A focusable Button (we abuse the Button widget by setting
        // .focusable(true) — TextEdit doesn't exist yet) takes focus
        // when pressed. Under PreserveOnMiss, pressing on empty
        // surface afterwards keeps focus.
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::input::PointerButton;
        use crate::layout::types::sizing::Sizing;
        use crate::support::testing::{begin, click_at};
        use crate::widgets::{button::Button, panel::Panel};

        let mut ui = Ui::new();
        ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert_eq!(
            ui.focused_id(),
            Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
        );

        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase(); // Press past the focusable rect.
        ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(180.0, 5.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
        assert_eq!(
            ui.focused_id(),
            Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
            "PreserveOnMiss keeps focus when press lands off any focusable widget",
        );
    }

    #[test]
    fn default_policy_is_clear_on_miss() {
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::input::PointerButton;
        use crate::layout::types::sizing::Sizing;
        use crate::support::testing::{begin, click_at};
        use crate::widgets::{button::Button, panel::Panel};

        // Pin: a fresh Ui starts with FocusPolicy::ClearOnMiss
        // (click-outside-to-blur is the native-app convention).
        let mut ui = Ui::new();
        assert_eq!(ui.focus_policy(), crate::FocusPolicy::ClearOnMiss);

        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert!(ui.focused_id().is_some());

        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        ui.on_input(InputEvent::PointerMoved(glam::Vec2::new(180.0, 5.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
        assert_eq!(
            ui.focused_id(),
            None,
            "default ClearOnMiss drops focus on a press past the focusable",
        );
    }

    #[test]
    fn clicking_non_focusable_widget_preserves_focus_under_preserve_policy() {
        // Two widgets: one focusable, one only clickable. Under
        // PreserveOnMiss, clicking the pure-Click widget shouldn't
        // steal focus from the focusable one. (Under default
        // ClearOnMiss this isn't true — the press lands on a
        // non-focusable widget and clears focus.)
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::layout::types::sizing::Sizing;
        use crate::support::testing::{begin, click_at};
        use crate::widgets::{button::Button, panel::Panel};

        let mut ui = Ui::new();
        ui.set_focus_policy(crate::FocusPolicy::PreserveOnMiss);
        begin(&mut ui, glam::UVec2::new(400, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
            Button::new()
                .id_salt("plain")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert_eq!(
            ui.focused_id(),
            Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
        );

        begin(&mut ui, glam::UVec2::new(400, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
            Button::new()
                .id_salt("plain")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        // Click the plain button — it captures the click but isn't
        // focusable, so focus stays on "editable".
        click_at(&mut ui, glam::Vec2::new(150.0, 20.0));
        assert_eq!(
            ui.focused_id(),
            Some(crate::forest::widget_id::WidgetId::from_hash("editable")),
            "click on non-focusable widget must not steal focus",
        );
    }

    #[test]
    fn focus_is_evicted_when_widget_disappears() {
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::layout::types::sizing::Sizing;
        use crate::support::testing::{begin, click_at};
        use crate::widgets::{button::Button, panel::Panel};

        let mut ui = Ui::new();
        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert!(ui.focused_id().is_some());

        // Next frame omits the focusable widget entirely.
        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |_ui| {});
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        assert_eq!(
            ui.focused_id(),
            None,
            "focused widget removed from tree must drop focus",
        );
    }

    #[test]
    fn request_focus_bypasses_policy() {
        use crate::Ui;
        let mut ui = Ui::new();
        let id = crate::forest::widget_id::WidgetId::from_hash("manual");
        ui.request_focus(Some(id));
        assert_eq!(ui.focused_id(), Some(id));
        ui.request_focus(None);
        assert_eq!(ui.focused_id(), None);
    }

    #[test]
    fn invisible_focusable_widget_does_not_take_focus() {
        // Cascade rule: invisible nodes drop their focusable bit just
        // like they drop their `Sense`. Pin separately from the
        // disabled case because the cascade combines `disabled ||
        // invisible` and a future split would silently keep one bit
        // alive.
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::forest::visibility::Visibility;
        use crate::layout::types::sizing::Sizing;
        use crate::support::testing::{begin, click_at};
        use crate::widgets::{button::Button, panel::Panel};

        let mut ui = Ui::new();
        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .visibility(Visibility::Hidden)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert_eq!(
            ui.focused_id(),
            None,
            "invisible focusable widget refuses focus",
        );
    }

    #[test]
    fn disabled_focusable_widget_does_not_take_focus() {
        // Cascade rule: disabled (or invisible) nodes drop their focusable
        // bit just like they drop their `Sense` — keystrokes shouldn't
        // route to a greyed-out field.
        use crate::Ui;
        use crate::forest::element::Configure;
        use crate::layout::types::sizing::Sizing;
        use crate::support::testing::{begin, click_at};
        use crate::widgets::{button::Button, panel::Panel};

        let mut ui = Ui::new();
        begin(&mut ui, glam::UVec2::new(200, 80));
        Panel::hstack().auto_id().show(&mut ui, |ui| {
            Button::new()
                .id_salt("editable")
                .focusable(true)
                .disabled(true)
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        click_at(&mut ui, glam::Vec2::new(50.0, 20.0));
        assert_eq!(
            ui.focused_id(),
            None,
            "disabled focusable widget refuses focus",
        );
    }

    #[test]
    fn end_frame_clears_keys_and_text_but_preserves_modifiers() {
        let mut state = InputState::new();
        let cascades = CascadeResult::default();
        state.on_input(
            InputEvent::ModifiersChanged(Modifiers {
                shift: true,
                ..Modifiers::NONE
            }),
            &cascades,
        );
        state.on_input(
            InputEvent::KeyDown {
                key: Key::ArrowLeft,
                repeat: false,
            },
            &cascades,
        );
        state.on_input(InputEvent::Text(TextChunk::new("x").unwrap()), &cascades);
        let key_cap_before = state.frame_keys.capacity();
        let text_cap_before = state.frame_text.capacity();

        state.end_frame(&cascades);

        assert!(state.frame_keys.is_empty());
        assert!(state.frame_text.is_empty());
        // Capacity-retained: typing across frames stays alloc-free in
        // steady state.
        assert_eq!(state.frame_keys.capacity(), key_cap_before);
        assert_eq!(state.frame_text.capacity(), text_cap_before);
        // Modifier state is a running snapshot, not per-frame — held
        // shift across frames must remain `true`.
        assert!(state.modifiers.shift);
    }
}

/// `ResponseState.focused` and `ResponseState.disabled` are
/// recently-added fields; pin both their semantics through the live
/// `Ui::response_for` path.
mod response_state {
    use super::*;
    use crate::forest::widget_id::WidgetId;
    use crate::widgets::frame::Frame;

    fn focusable_id() -> WidgetId {
        WidgetId::from_hash("focusable")
    }

    fn build_focusable_leaf(ui: &mut Ui) {
        Frame::new()
            .id_salt("focusable")
            .focusable(true)
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .show(ui);
    }

    /// `state.focused` reflects `Ui::input.focused == Some(id)`. No
    /// one-frame lag — `request_focus` lands the same frame, unlike
    /// hover/press which read the prior frame's cascade.
    #[test]
    fn focused_reflects_focused_id_synchronously() {
        let mut ui = ui_at(UVec2::new(200, 200));
        build_focusable_leaf(&mut ui);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        // Initially nothing is focused.
        assert!(!ui.response_for(focusable_id()).focused);

        // request_focus updates state synchronously.
        ui.request_focus(Some(focusable_id()));
        assert!(
            ui.response_for(focusable_id()).focused,
            "focused must be true the same frame as request_focus",
        );

        // Clearing focus drops it.
        ui.request_focus(None);
        assert!(!ui.response_for(focusable_id()).focused);
    }

    /// `state.disabled` is the cascaded ancestor-or-self flag from
    /// the *previous* frame's cascade. A child of a disabled parent
    /// reads `disabled = true` even though the child's own
    /// `Element::disabled` is false. Lag is acceptable (matches
    /// hover/press) — widgets that need lag-free self-toggle merge
    /// `state.disabled |= element.disabled` themselves.
    #[test]
    fn disabled_reflects_cascaded_ancestor_flag() {
        let mut ui = ui_at(UVec2::new(200, 200));
        // Frame 0: parent disabled, child not. Cascade marks both.
        Panel::vstack()
            .id_salt("parent")
            .disabled(true)
            .show(&mut ui, |ui| {
                Frame::new()
                    .id_salt("child")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui);
            });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        // Re-record so cascade is populated for response_for to read.
        begin(&mut ui, UVec2::new(200, 200));
        Panel::vstack()
            .id_salt("parent")
            .disabled(true)
            .show(&mut ui, |ui| {
                Frame::new()
                    .id_salt("child")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui);
            });

        let parent_state = ui.response_for(WidgetId::from_hash("parent"));
        let child_state = ui.response_for(WidgetId::from_hash("child"));
        assert!(
            parent_state.disabled,
            "parent's own disabled flag flows into ResponseState",
        );
        assert!(
            child_state.disabled,
            "child inherits cascaded disabled from parent (no self flag)",
        );
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
    }

    /// Conversely, with no disabled in the chain, child's
    /// `state.disabled` stays false.
    #[test]
    fn disabled_false_when_chain_clean() {
        let mut ui = ui_at(UVec2::new(200, 200));
        Panel::vstack().id_salt("parent").show(&mut ui, |ui| {
            Frame::new()
                .id_salt("child")
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui);
        });
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
        begin(&mut ui, UVec2::new(200, 200));
        Panel::vstack().id_salt("parent").show(&mut ui, |ui| {
            Frame::new()
                .id_salt("child")
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui);
        });
        assert!(!ui.response_for(WidgetId::from_hash("child")).disabled);
        ui.end_frame_record_phase();
        ui.end_frame_paint_phase();
    }
}
