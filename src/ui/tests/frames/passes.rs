//! Pass count and replay: what triggers a second record, and what must run
//! only once.

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

/// Cascade runs in `post_record` (after each pass's measure+arrange),
/// not in `finalize_frame`. Means a `request_relayout` re-record can
/// read pass A's arranged rect via `response_for(id).rect` — the
/// invariant `ContextMenu::show` relies on to clamp its anchor in
/// the same frame as the first open, and the general API contract
/// for any widget that needs its own size mid-frame.
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
                // Pass A: no cascade yet for our frame this run — first
                // ever recording of this widget. Trigger pass B.
                pass_a_rect.set(resp.state.rect);
                ui.request_relayout();
            }
            1 => {
                // Pass B: cascade was rebuilt by pass A's post_record,
                // so response_for now returns pass A's arranged rect.
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

/// `Ui::frame` re-records when the frame contained routed input that could
/// drive a state mutation, and runs the build closure exactly once
/// otherwise.
/// Action coverage has to be exact: false positives waste CPU silently,
/// false
/// negatives leave the popup-dismissal class of bugs unfixed.
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
        // The press frame first: a press lands a frame before its
        // release, and the release frame is the one measured.
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
        // Baseline frame so the under-test `frame` diffs against a real
        // prior recording, not the never-painted initial state.
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
        // The render frame id must bump exactly once per `frame`
        // regardless of pass count — pass B's anim ticks must see the same
        // id as pass A's so the integrator doesn't double-advance.
        assert_eq!(
            h.ui.frame_runtime.render_frame_id,
            render_frame_before + 1,
            "{label}: render_frame_id must bump once per frame (passes: {expected})",
        );
    }
}

/// A routed action requests pass B, but its edge is visible only in pass A.
/// This lets application code handle a widget action inline without
/// replaying
/// the effect.
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

/// A relayout request forces a second record pass, exactly as pending
/// action input does. `frame_value` still records both — skipping pass B
/// would leave an empty tree — but hands back pass A's value, because
/// pass A is the one that observes one-frame edges.
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

/// `Ui::frame` plumbs `now`, `dt`, and the repaint-requested flag
/// end-to-end: per-call `now` lands in the frame runtime, the derived `dt`
/// clamps to `MAX_ANIM_DT`, `repaint_requested` resets at the top of every
/// call, and a flag set during recording surfaces on `FrameOutput`.
#[test]
fn frame_plumbs_now_dt_and_repaint_request() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    // Frame A: idle, no repaint request, now = 16ms.
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

    // Frame B: simulate an unsettled animation tick by setting the
    // internal flag during recording. The flag must reach `FrameOutput`.
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

    // Frame C: oversized gap (5s) clamps dt to MAX_ANIM_DT; `time` still
    // tracks true clock so animation math doesn't teleport.
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

    // Frame D: prior frame's repaint_requested must NOT leak — resets
    // at the top of every `frame` regardless of pass count.
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

/// `App::update` reads responses before any record pass of its frame, so
/// it needs its own quiescence snapshot. Here the last pass ran with the
/// pointer off the surface, and the press arrived after it: a snapshot
/// left from that pass would default the whole interaction half out.
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
