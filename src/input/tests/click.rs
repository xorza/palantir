use crate::Ui;
use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::widgets::{button::Button, panel::Panel};
use glam::{UVec2, Vec2};

#[test]
fn input_state_press_release_emits_click() {
    // Frame 1 lays out the button; frame 2 reads .clicked() after a
    // press+release pair lands inside its rect; frame 3 confirms the
    // click is one-shot.
    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 80);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id_salt("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    };
    ui.run_at_acked(surface, build);
    ui.click_at(Vec2::new(50.0, 20.0));

    let mut got_click = false;
    ui.run_at_acked(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            got_click |= Button::new()
                .id_salt("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui)
                .clicked();
        });
    });
    assert!(got_click, "press+release inside button rect should click");

    let mut still_clicking = false;
    ui.run_at_acked(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            still_clicking |= Button::new()
                .id_salt("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui)
                .clicked();
        });
    });
    assert!(!still_clicking, "click is one-shot");
}

#[test]
fn stack_sense_routing() {
    // (label, sense, click_pos, expects_stack_click, expects_stack_hover, expects_child_click).
    let cases: &[(&str, Sense, Vec2, bool, bool, bool)] = &[
        (
            "sense_none_passes_through",
            Sense::NONE,
            Vec2::new(5.0, 5.0),
            false,
            false,
            false,
        ),
        (
            "sense_click_captures_background",
            Sense::CLICK,
            Vec2::new(5.0, 5.0),
            true,
            true,
            false,
        ),
        (
            "sense_hover_reports_hover_only",
            Sense::HOVER,
            Vec2::new(5.0, 5.0),
            false,
            true,
            false,
        ),
    ];
    for (label, sense, click_pos, expect_stack_click, expect_stack_hover, expect_child_click) in
        cases
    {
        let mut ui = Ui::for_test();
        let surface = UVec2::new(200, 100);
        let build = |ui: &mut Ui| {
            Panel::hstack()
                .id_salt("stack")
                .padding(20.0)
                .sense(*sense)
                .show(ui, |ui| {
                    Button::new()
                        .id_salt("inside")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                        .show(ui);
                });
        };
        ui.run_at_acked(surface, build);
        ui.click_at(*click_pos);

        let mut child_clicked = false;
        let mut stack_clicked = false;
        let mut stack_hovered = false;
        ui.run_at_acked(surface, |ui| {
            let r = Panel::hstack()
                .id_salt("stack")
                .padding(20.0)
                .sense(*sense)
                .show(ui, |ui| {
                    child_clicked |= Button::new()
                        .id_salt("inside")
                        .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                        .show(ui)
                        .clicked();
                });
            stack_clicked |= r.clicked();
            stack_hovered |= r.hovered();
        });
        assert_eq!(
            stack_clicked, *expect_stack_click,
            "case {label}: stack clicked"
        );
        assert_eq!(
            stack_hovered, *expect_stack_hover,
            "case {label}: stack hovered"
        );
        assert_eq!(
            child_clicked, *expect_child_click,
            "case {label}: child clicked"
        );
    }
}

#[test]
fn input_state_release_outside_does_not_click() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(400, 80);
    ui.run_at_acked(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Button::new()
                .id_salt("target")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        });
    });
    ui.press_at(Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 20.0)));
    ui.release_left();

    let mut got_click = false;
    ui.run_at_acked(surface, |ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            got_click |= Button::new()
                .id_salt("target")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui)
                .clicked();
        });
    });
    assert!(
        !got_click,
        "release outside the original widget cancels click"
    );
}

#[test]
fn click_on_overflow_outside_clipped_parent_is_suppressed() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(400, 400);
    let build = |ui: &mut Ui, capture: &mut bool| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::zstack()
                .id_salt("clipper")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
                .clip_rect()
                .show(ui, |ui| {
                    *capture |= Button::new()
                        .id_salt("inner")
                        .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                        .show(ui)
                        .clicked();
                });
        });
    };
    let mut sink = false;
    ui.run_at_acked(surface, |ui| build(ui, &mut sink));
    ui.click_at(Vec2::new(150.0, 150.0));

    let mut clicked = false;
    ui.run_at_acked(surface, |ui| build(ui, &mut clicked));
    assert!(
        !clicked,
        "click on overflow outside clip should not register"
    );
}

#[test]
fn zoom_panel_routes_clicks_by_world_rect() {
    use crate::primitives::transform::TranslateScale;
    // (label, scale, click_pos, expect_hit).
    let cases: &[(&str, f32, Vec2, bool)] = &[
        ("scale_2x_inside", 2.0, Vec2::new(75.0, 75.0), true),
        (
            "scale_0.5x_outside_world",
            0.5,
            Vec2::new(40.0, 40.0),
            false,
        ),
    ];
    for (label, scale, click_pos, expect) in cases {
        let mut ui = Ui::for_test();
        let surface = UVec2::new(400, 400);
        let build = |ui: &mut Ui, capture: &mut bool| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::zstack()
                    .id_salt("zoomer")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .transform(TranslateScale::from_scale(*scale))
                    .show(ui, |ui| {
                        *capture |= Button::new()
                            .id_salt("inner")
                            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                            .show(ui)
                            .clicked();
                    });
            });
        };
        let mut sink = false;
        ui.run_at_acked(surface, |ui| build(ui, &mut sink));
        ui.click_at(*click_pos);

        let mut clicked = false;
        ui.run_at_acked(surface, |ui| build(ui, &mut clicked));
        assert_eq!(clicked, *expect, "case {label}");
    }
}

#[test]
fn secondary_click_press_release_emits_secondary_clicked() {
    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 80);
    let build = |ui: &mut Ui, sink: &mut bool| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let r = Button::new()
                .id_salt("rc_target")
                .label("rc")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
            *sink |= r.secondary_clicked();
            // Left-click must NOT flip secondary_clicked.
            assert!(!(r.clicked() && r.secondary_clicked()));
        });
    };
    let mut sink = false;
    ui.run_at_acked(surface, |ui| build(ui, &mut sink));
    ui.secondary_click_at(Vec2::new(50.0, 20.0));

    let mut got = false;
    ui.run_at_acked(surface, |ui| build(ui, &mut got));
    assert!(got, "right press+release should set secondary_clicked");

    // One-shot.
    let mut still = false;
    ui.run_at_acked(surface, |ui| build(ui, &mut still));
    assert!(!still, "secondary_clicked is one-shot");
}

#[test]
fn left_and_right_click_are_independent() {
    use crate::input::pointer::PointerButton;
    let mut ui = Ui::for_test();
    let surface = UVec2::new(200, 80);
    let build = |ui: &mut Ui, lc: &mut bool, rc: &mut bool| {
        Panel::hstack().auto_id().show(ui, |ui| {
            let r = Button::new()
                .id_salt("indep")
                .label("x")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
            *lc |= r.clicked();
            *rc |= r.secondary_clicked();
        });
    };
    let mut a = false;
    let mut b = false;
    ui.run_at_acked(surface, |ui| build(ui, &mut a, &mut b));

    // Left-press, then a right press+release while left is still held —
    // both should latch separately.
    ui.press_at(Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Right));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Right));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

    let mut lc = false;
    let mut rc = false;
    ui.run_at_acked(surface, |ui| build(ui, &mut lc, &mut rc));
    assert!(lc, "left click should still fire");
    assert!(rc, "right click should still fire alongside left");
}
