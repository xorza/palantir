//! Pass count and replay: what triggers a second record.

use crate::Ui;
use crate::common::time::MAX_ANIM_DT;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::tests::support::SURFACE;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::ResponseSnapshot;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::{UVec2, Vec2};
use std::cell::{Cell, RefCell};
use std::time::Duration;

/// Cascade runs in `post_record`, not `finalize_frame`, so a `request_relayout` re-record can read pass A's rect via `response_for(id).rect` (`ContextMenu::show` clamps its anchor with it).
#[test]
fn cascade_visible_to_relayout_pass() {
    let pass = Cell::new(0u32);
    let pass_a_rect = Cell::new(None::<Rect>);
    let pass_b_rect = Cell::new(None::<Rect>);
    let id_salt = "cascade-relayout-probe";

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        let probe_resp: RefCell<Option<ResponseSnapshot>> = RefCell::new(None);
        Panel::vstack().auto_id().show(ui, |ui| {
            *probe_resp.borrow_mut() = Some(
                Block::new()
                    .id(WidgetId::from_hash(id_salt))
                    .size(40.0)
                    .show(ui)
                    .snapshot(),
            );
        });
        let resp = probe_resp.into_inner().unwrap();
        match pass.get() {
            0 => {
                // Pass A: no cascade yet; triggers pass B.
                pass_a_rect.set(resp.state.rect);
                ui.request_relayout();
            }
            1 => {
                // Pass B: `response_for` returns pass A's arranged rect.
                pass_b_rect.set(resp.state.rect);
            }
            _ => unreachable!("relayout capped at one retry per frame"),
        }
        pass.set(pass.get() + 1);
    });

    assert_eq!(pass.get(), 2, "expected exactly two record passes");
    assert!(
        pass_a_rect.get().is_none(),
        "pass A sees no cascade entry yet (widget first recorded this frame)",
    );
    let b = pass_b_rect.get().expect("pass B reads pass-A cascade");
    assert_eq!(b.size.w, 40.0);
    assert_eq!(b.size.h, 40.0);
}

/// `Ui::frame` re-records when the frame had routed input that could drive a state mutation, else runs the closure once.
#[test]
fn frame_pass_count_matches_action_trigger() {
    use crate::input::input_event::InputEvent;
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::modifiers::Modifiers;

    use crate::input::sense::Sense;
    use crate::primitives::layout::sizing::Sizing;
    use glam::Vec2;

    fn build_target(ui: &mut Ui) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .sense(Sense::CLICK)
            .focusable(true)
            .show(ui, |_| {});
    }

    type Prime = fn(&mut UiHarness);
    let cases: &[(&str, Prime, usize)] = &[
        ("idle", |_ui| {}, 1),
        (
            "hover only",
            |h| {
                h.move_to(Vec2::new(10.0, 10.0));
            },
            1,
        ),
        (
            "modifiers only",
            |h| {
                h.on_input(InputEvent::ModifiersChanged(Modifiers::NONE));
            },
            1,
        ),
        (
            "routed click",
            |h| {
                h.press_at(Vec2::new(10.0, 10.0));
                h.frame(build_target);
                h.release();
            },
            2,
        ),
        ("unrouted click", |h| h.click_at(Vec2::new(150.0, 150.0)), 1),
        (
            "unrouted keydown",
            |h| {
                h.key(Key::Enter);
            },
            1,
        ),
        (
            "routed keydown",
            |h| {
                h.set_focus(WidgetId::from_hash("root"));
                h.key(Key::Enter);
            },
            2,
        ),
        (
            "scroll",
            |h| {
                h.scroll_pixels(Vec2::new(0.0, 10.0));
            },
            1,
        ),
    ];

    for (label, prime, expected) in cases {
        let mut h = UiHarness::new(UVec2::new(100, 100));
        h.frame(build_target);
        prime(&mut h);

        let count = Cell::new(0u32);
        let render_frame_before = h.ui.frame_runtime.render_frame_id;
        let _ = h.frame(|ui| {
            count.set(count.get() + 1);
            build_target(ui);
        });
        assert_eq!(
            count.get() as usize,
            *expected,
            "{label}: expected {expected} build invocation(s), got {}",
            count.get(),
        );
        // The render frame id bumps once per `frame`, so pass B's anim ticks see pass A's id.
        assert_eq!(
            h.ui.frame_runtime.render_frame_id,
            render_frame_before + 1,
            "{label}: render_frame_id must bump once per frame (passes: {expected})",
        );
    }
}

/// A routed action requests pass B but its edge is visible only in pass A.
#[test]
fn action_effect_runs_once_across_record_replay() {
    let surface = UVec2::new(100, 100);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Button::new()
            .id(WidgetId::from_hash("action"))
            .label("Run")
            .size((100.0, 100.0))
            .show(ui)
            .left
            .clicked()
    };

    h.frame(|ui| {
        let _ = build(ui);
    });
    h.press_at(Vec2::new(10.0, 10.0));
    h.frame(|ui| {
        let _ = build(ui);
    });
    h.release();

    let clicks = h.at(Duration::from_millis(16)).frame_passes(build);
    assert_eq!(clicks.len(), 2, "action input must request a replay pass");
    assert_eq!(
        (*clicks.a(), clicks.b()),
        (true, Some(&false)),
        "the action edge shows in pass A and does not replay"
    );
}

/// A relayout request forces a second record pass; `frame_value` returns pass A's value, which observes one-frame edges.
#[test]
fn frame_value_records_both_relayout_passes_and_returns_the_first() {
    let mut h = UiHarness::new(SURFACE);
    let mut calls = 0_u32;

    let captured = h.frame_value(|ui| {
        calls += 1;
        if calls == 1 {
            ui.request_relayout();
        }
        calls
    });

    assert_eq!(calls, 2, "relayout runs exactly two record passes");
    assert_eq!(captured, 1, "capture returns the input-observing pass");
}

/// `Ui::frame` plumbs `now`, `dt` (clamped to `MAX_ANIM_DT`) and the repaint-requested flag (reset every call) end-to-end to `FrameOutput`.
#[test]
fn frame_plumbs_now_dt_and_repaint_request() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    let repaint = h
        .at(Duration::from_millis(16))
        .frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        })
        .repaint_requested;
    assert!(
        !repaint,
        "no animate-not-settled flag set — must stay false"
    );
    assert_eq!(h.ui.frame_runtime.time, Duration::from_millis(16));
    assert_eq!(
        h.ui.frame_runtime.dt,
        Duration::from_millis(16).as_secs_f32(),
        "FrameRuntime::dt should be (now - prev) in seconds",
    );

    // Frame B: an unsettled animation tick during recording must reach `FrameOutput`.
    let repaint = h
        .at(Duration::from_millis(32))
        .frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
            ui.frame_runtime.repaint_requested = true;
        })
        .repaint_requested;
    assert!(
        repaint,
        "repaint_requested set during recording must surface on FrameOutput",
    );
    assert_eq!(h.ui.frame_runtime.time, Duration::from_millis(32));
    assert_eq!(
        h.ui.frame_runtime.dt,
        Duration::from_millis(16).as_secs_f32(),
        "FrameRuntime::dt should be next-frame delta",
    );

    // Frame C: a 5s gap clamps dt to MAX_ANIM_DT; `time` still tracks the true clock.
    let _ = h.at(Duration::from_millis(5_032)).frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    assert_eq!(h.ui.frame_runtime.time, Duration::from_millis(5_032));
    assert_eq!(
        h.ui.frame_runtime.dt, MAX_ANIM_DT,
        "FrameRuntime::dt should clamp at MAX_ANIM_DT",
    );

    // Frame D: the prior frame's repaint_requested must not leak.
    let repaint = h
        .at(Duration::from_millis(5_048))
        .frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        })
        .repaint_requested;
    assert!(
        !repaint,
        "repaint_requested must reset at the top of frame()",
    );
}

/// `App::update` reads responses before any record pass, so it needs its own quiescence snapshot: a stale one from a pass with the pointer off the surface would default the interaction half out.
#[test]
fn update_sees_input_that_arrived_after_a_quiescent_pass() {
    use crate::app::App;
    use crate::window::window_token::WindowToken;

    #[derive(Debug)]
    struct Probe {
        id: WidgetId,
        seen: Vec<bool>,
    }

    impl App for Probe {
        fn update(&mut self, _win: WindowToken, ui: &Ui) {
            let response = ui.response_for(self.id);
            self.seen.push(response.pressed() && response.pointer_over);
        }

        fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
            Button::new().id(self.id).size(40.0).show(ui);
        }
    }

    let mut h = UiHarness::new(SURFACE);
    let mut probe = Probe {
        id: WidgetId::from_hash("update-reads-press"),
        seen: Vec::new(),
    };
    h.frame_app(&mut probe);
    assert_eq!(probe.seen, [false], "nothing pressed yet");

    h.press_at(Vec2::new(20.0, 20.0));
    h.frame_app(&mut probe);
    assert_eq!(probe.seen, [false, true], "the press reaches update");
}
