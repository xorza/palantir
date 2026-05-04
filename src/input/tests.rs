use crate::Ui;
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{display::Display, sense::Sense, sizing::Sizing};
use crate::support::testing::{begin, click_at, press_at, release_left, ui_at};
use crate::tree::element::Configure;
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
        .show(&mut ui, |ui| {
            Button::new()
                .with_id("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    // Press inside the button, release inside.
    click_at(&mut ui, Vec2::new(50.0, 20.0));

    // Frame 2: rebuild; widgets should observe the click in build_ui.
    begin(&mut ui, UVec2::new(200, 80));
    let mut got_click = false;
    Panel::hstack().show(&mut ui, |ui| {
        let r = Button::new()
            .with_id("target")
            .label("hi")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
            .show(ui);
        got_click = r.clicked();
    });
    assert!(got_click, "press+release inside button rect should click");

    // Click does not stick: next frame without input must clear it.
    ui.end_frame();
    begin(&mut ui, UVec2::new(200, 80));
    let mut still_clicking = false;
    Panel::hstack().show(&mut ui, |ui| {
        still_clicking = Button::new()
            .with_id("target")
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
        .padding(20.0) // creates "background" area to click
        .show(&mut ui, |ui| {
            Button::new()
                .with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    // Press inside the HStack's padding (not over any child).
    click_at(&mut ui, Vec2::new(5.0, 5.0));

    ui.begin_frame(Display::default());
    let mut child_clicked = false;
    let stack_resp = Panel::hstack().padding(20.0).show(&mut ui, |ui| {
        child_clicked = Button::new()
            .with_id("inside")
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
        .with_id("clickable_card")
        .padding(20.0)
        .sense(Sense::CLICK)
        .show(&mut ui, |ui| {
            Button::new()
                .with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    click_at(&mut ui, Vec2::new(5.0, 5.0));

    ui.begin_frame(Display::default());
    let stack_resp = Panel::hstack()
        .with_id("clickable_card")
        .padding(20.0)
        .sense(Sense::CLICK)
        .show(&mut ui, |ui| {
            Button::new()
                .with_id("inside")
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
        .with_id("hover_only")
        .padding(20.0)
        .sense(Sense::HOVER)
        .show(&mut ui, |ui| {
            Button::new()
                .with_id("inside")
                .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    // Move pointer over stack's padding area (not over the button).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(5.0, 5.0)));

    // Press + release on the same spot.
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    release_left(&mut ui);

    ui.begin_frame(Display::default());
    let mut child_clicked = false;
    let stack_resp = Panel::hstack()
        .with_id("hover_only")
        .padding(20.0)
        .sense(Sense::HOVER)
        .show(&mut ui, |ui| {
            child_clicked = Button::new()
                .with_id("inside")
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
        .show(&mut ui, |ui| {
            Button::new()
                .with_id("target")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    press_at(&mut ui, Vec2::new(50.0, 20.0));
    ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 20.0))); // outside
    release_left(&mut ui);

    ui.begin_frame(Display::default());
    let mut got_click = false;
    Panel::hstack().show(&mut ui, |ui| {
        got_click = Button::new()
            .with_id("target")
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
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("clipper")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .clip(true)
            .show(ui, |ui| {
                Button::new()
                    .with_id("inner")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(200.0)))
                    .show(ui);
            });
    });
    ui.end_frame();

    // Click well outside the panel's 100x100 clip rect but inside the button's
    // raw 200x200 rect (overflow region). With clip-aware hit-test this misses.
    click_at(&mut ui, Vec2::new(150.0, 150.0));

    // Frame 2: read .clicked() from the button's response.
    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("clipper")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .clip(true)
            .show(ui, |ui| {
                clicked = Button::new()
                    .with_id("inner")
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
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(2.0))
            .clip(false)
            .show(ui, |ui| {
                Button::new()
                    .with_id("inner")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui);
            });
    });
    ui.end_frame();

    // Click at world (75, 75) — inside the zoomed 100x100 bounds.
    click_at(&mut ui, Vec2::new(75.0, 75.0));

    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(2.0))
            .clip(false)
            .show(ui, |ui| {
                clicked = Button::new()
                    .with_id("inner")
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
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(0.5))
            .show(ui, |ui| {
                Button::new()
                    .with_id("inner")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui);
            });
    });
    ui.end_frame();

    // Button's world rect under scale=0.5 is 25x25. Click at (40, 40) is
    // inside the LOGICAL rect but outside the world-rendered rect.
    click_at(&mut ui, Vec2::new(40.0, 40.0));

    ui.begin_frame(Display::default());
    let mut clicked = false;
    Panel::hstack().show(&mut ui, |ui| {
        Panel::zstack()
            .with_id("zoomer")
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .transform(TranslateScale::from_scale(0.5))
            .show(ui, |ui| {
                clicked = Button::new()
                    .with_id("inner")
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui)
                    .clicked();
            });
    });
    assert!(!clicked, "click outside world-rendered bounds should miss");
}
