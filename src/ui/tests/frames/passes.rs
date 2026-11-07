//! Pass count and replay: what triggers a second record.

use crate::theme::color_scheme::ColorScheme;

use crate::common::sync::MutexExt;
use crate::common::time::MAX_ANIMATION_DT;
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::sense::Sense;
use crate::ui::Ui;
use crate::ui::app::{App, InputOutcome};
use crate::ui::frame_report::FrameProcessing;
use crate::ui::frame_runtime::MAX_CARRIED_RELAYOUTS;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::tests::support::{self, SURFACE};
use crate::ui::update_ctx::UpdateCtx;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::cell::{Cell, RefCell};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::subscriber;

/// A pass opens with no settling pass asked, so an ask counts only in the pass that made it: the
/// warmup's is withdrawn, pass A's adds pass B, and a first frame runs at most those three.
#[test]
fn a_relayout_ask_counts_in_its_own_pass() {
    for (asking, passes, processing) in [
        (&[][..], 2, FrameProcessing::SingleLayout),
        (&[0][..], 2, FrameProcessing::SingleLayout),
        (&[1][..], 3, FrameProcessing::DoubleLayout),
        (&[0, 1][..], 3, FrameProcessing::DoubleLayout),
    ] {
        let mut h = Harness::cold(SURFACE);
        let mut pass = 0;
        let report = h.frame(|ui| {
            if asking.contains(&pass) {
                ui.request_relayout();
            }
            pass += 1;
        });
        assert_eq!(pass, passes, "asking in passes {asking:?}");
        assert_eq!(report.processing, processing, "asking in passes {asking:?}");
        assert!(
            !report.repaint_requested,
            "the last pass asked nothing, so nothing carries: asking in passes {asking:?}"
        );
    }
}

/// Cascade runs in `post_record`, not `finalize_frame`, so a `request_relayout` re-record can read pass A's rect via `response_for(id).rect` (`Menu::show` clamps its anchor with it).
#[test]
fn cascade_visible_to_relayout_pass() {
    let pass = Cell::new(0u32);
    let pass_a_rect = Cell::new(None::<Rect>);
    let pass_b_rect = Cell::new(None::<Rect>);
    let id_salt = "cascade-relayout-probe";

    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        let probe_resp: RefCell<Option<Response>> = RefCell::new(None);
        Panel::vstack().auto_id().show(ui, |ui| {
            *probe_resp.borrow_mut() = Some(
                Block::new()
                    .id(WidgetId::from_hash(id_salt))
                    .size(40.0)
                    .show(ui),
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

/// `WindowFrames::frame` re-records when the frame had routed input that could drive a state mutation, else runs the closure once.
#[test]
fn frame_pass_count_matches_action_trigger() {
    use crate::primitives::input::input_event::InputEvent;
    use crate::primitives::input::keyboard::key::Key;
    use crate::primitives::input::keyboard::modifiers::Modifiers;

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

    type Prime = fn(&mut Harness);
    let cases: &[(&str, Prime, usize)] = &[
        ("idle", |_ui| {}, 1),
        (
            "hover only",
            |h| {
                h.hover(Vec2::new(10.0, 10.0));
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
                h.press(Vec2::new(10.0, 10.0), PointerButton::Left);
                h.frame(build_target);
                h.release(PointerButton::Left);
            },
            2,
        ),
        (
            "unrouted click",
            |h| {
                h.click(Vec2::new(150.0, 150.0));
            },
            1,
        ),
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
                h.ui_mut().set_focus(WidgetId::from_hash("root"));
                h.key(Key::Enter);
            },
            2,
        ),
        (
            "scroll",
            |h| {
                h.on_input(InputEvent::ScrollPixels(Vec2::new(0.0, 10.0)));
            },
            1,
        ),
    ];

    for (label, prime, expected) in cases {
        let mut h = Harness::mono(UVec2::new(100, 100));
        h.frame(build_target);
        prime(&mut h);

        let count = Cell::new(0u32);
        let render_frame_before = h.ui().frame_runtime.render_frame_id;
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
            h.ui().frame_runtime.render_frame_id,
            render_frame_before + 1,
            "{label}: render_frame_id must bump once per frame (passes: {expected})",
        );
    }
}

/// A routed action requests pass B but its edge is visible only in pass A.
#[test]
fn action_effect_runs_once_across_record_replay() {
    let surface = UVec2::new(100, 100);
    let mut h = Harness::mono(surface);
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
    h.press(Vec2::new(10.0, 10.0), PointerButton::Left);
    h.frame(|ui| {
        let _ = build(ui);
    });
    h.release(PointerButton::Left);

    let clicks = h.at(Duration::from_millis(16)).frame_passes(build);
    assert_eq!(clicks.len(), 2, "action input must request a replay pass");
    assert_eq!(
        (*clicks.a(), clicks.b()),
        (true, Some(&false)),
        "the action edge shows in pass A and does not replay"
    );
}

/// An app whose update takes the click on [`Self::BUTTON`] and answers `outcome`, and whose
/// record asks for a pass when `asks` is set.
#[derive(Debug)]
struct Handles {
    outcome: InputOutcome,
    asks: bool,
    passes: u32,
    seen_by_input: u32,
    /// What each record pass read of the click.
    seen_by_record: Vec<bool>,
}

impl Handles {
    const BUTTON: &str = "handles.button";

    const fn new(outcome: InputOutcome, asks: bool) -> Self {
        Self {
            outcome,
            asks,
            passes: 0,
            seen_by_input: 0,
            seen_by_record: Vec::new(),
        }
    }
}

impl App for Handles {
    fn update(&mut self, _: WindowToken, ctx: UpdateCtx<'_>) -> InputOutcome {
        if ctx
            .input
            .response_for(WidgetId::from_hash(Self::BUTTON))
            .left
            .clicked()
        {
            self.seen_by_input += 1;
        }
        self.outcome
    }

    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        self.passes += 1;
        let clicked = Button::new()
            .id(WidgetId::from_hash(Self::BUTTON))
            .label("Go")
            .size((100.0, 40.0))
            .show(ui)
            .left
            .clicked();
        self.seen_by_record.push(clicked);
        if self.asks && self.passes == 1 {
            ui.request_relayout();
        }
    }
}

/// One click by what the update returned: `Record` reads the edge in pass A and records again,
/// `Handled` takes the edge from every record and records once, and a record that asks gets its
/// pass all the same.
#[test]
fn a_click_records_by_the_update_answer() {
    let button = WidgetId::from_hash(Handles::BUTTON);
    for (outcome, asks, read) in [
        (InputOutcome::Record, false, &[true, false][..]),
        (InputOutcome::Handled, false, &[false][..]),
        (InputOutcome::Handled, true, &[false, false][..]),
    ] {
        let mut h = Harness::mono(SURFACE);
        let mut warm = Handles::new(InputOutcome::Record, false);
        h.frame_app(&mut warm);
        h.frame_app(&mut warm);
        h.press(button, PointerButton::Left);
        h.frame_app(&mut warm);
        h.release(PointerButton::Left);

        let mut app = Handles::new(outcome, asks);
        let report = h.frame_app(&mut app);
        assert_eq!(app.seen_by_input, 1, "{outcome:?}, asks {asks}");
        assert_eq!(app.seen_by_record, read, "{outcome:?}, asks {asks}");
        assert_eq!(
            report.processing,
            if read.len() == 2 {
                FrameProcessing::DoubleLayout
            } else {
                FrameProcessing::SingleLayout
            },
            "{outcome:?}, asks {asks}",
        );
    }
}

/// An app that reads a key in its record, whose update answers `outcome`.
struct KeyProbe {
    outcome: InputOutcome,
    read_in_record: bool,
}
impl App for KeyProbe {
    fn update(&mut self, _: WindowToken, _: UpdateCtx<'_>) -> InputOutcome {
        self.outcome
    }
    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        self.read_in_record |= ui.key_pressed(Key::Char('k'));
    }
}

/// An app that adds up a wheel's pan in its record, whose update answers `outcome`.
struct WheelProbe {
    outcome: InputOutcome,
    panned: Vec2,
}
impl App for WheelProbe {
    fn update(&mut self, _: WindowToken, _: UpdateCtx<'_>) -> InputOutcome {
        self.outcome
    }
    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        let response = Block::new()
            .id(WidgetId::from_hash("wheel"))
            .size(100.0)
            .sense(Sense::SCROLL)
            .show(ui);
        self.panned += response.scroll.pan(1.0);
    }
}

/// `Handled` takes edges and leaves levels: a press the stage handled keeps its capture, so the
/// click that ends it reaches a record that the next frame's stage leaves it to, and a key is gone
/// from the frame it arrived on.
#[test]
fn a_handled_frame_keeps_what_the_pointer_holds() {
    let button = WidgetId::from_hash(Handles::BUTTON);
    let mut h = Harness::mono(SURFACE);
    let mut handled = Handles::new(InputOutcome::Handled, false);
    h.frame_app(&mut handled);
    h.frame_app(&mut handled);
    h.press(button, PointerButton::Left);
    h.frame_app(&mut handled);
    assert!(h.ui().response_for(button).left.held(), "the capture holds");

    h.release(PointerButton::Left);
    let mut record = Handles::new(InputOutcome::Record, false);
    h.frame_app(&mut record);
    assert_eq!(record.seen_by_record, [true, false], "the click lands");

    for (outcome, want) in [(InputOutcome::Record, true), (InputOutcome::Handled, false)] {
        let mut h = Harness::mono(SURFACE);
        let mut probe = KeyProbe {
            outcome,
            read_in_record: false,
        };
        h.frame_app(&mut probe);
        h.key(Key::Char('k'));
        h.frame_app(&mut probe);
        assert_eq!(probe.read_in_record, want, "a key, {outcome:?}");
    }

    let wheel = WidgetId::from_hash("wheel");
    let delta = Vec2::new(0.0, 12.0);
    for (outcome, want) in [
        (InputOutcome::Record, delta),
        (InputOutcome::Handled, Vec2::ZERO),
    ] {
        let mut h = Harness::mono(SURFACE);
        let mut probe = WheelProbe {
            outcome,
            panned: Vec2::ZERO,
        };
        h.frame_app(&mut probe);
        h.frame_app(&mut probe);
        h.scroll_pixels(wheel, delta);
        h.frame_app(&mut probe);
        assert_eq!(probe.panned, want, "a wheel, {outcome:?}");
    }
}

/// What a test's subscriber writes.
#[derive(Clone, Debug, Default)]
struct LoopLog(Arc<Mutex<Vec<u8>>>);

impl io::Write for LoopLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock_unpoisoned().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A relayout asked in the last pass takes the next frame, so the ask is not lost. A frame that
/// asks only in pass A owes none. Asking in the last pass of [`MAX_CARRIED_RELAYOUTS`] frames in a
/// row is still allowed, and a frame that asks nothing starts the count again. One frame past it
/// is a loop: every build logs one error and asks no more frames, until a frame that asks nothing.
#[test]
fn a_relayout_in_the_last_pass_takes_the_next_frame_and_a_loop_is_an_error() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |_| {});

    let mut passes = 0;
    let report = h.step(|ui| {
        if passes == 0 {
            ui.request_relayout();
        }
        passes += 1;
    });
    assert_eq!(passes, 2);
    assert!(!report.repaint_requested, "pass B asked nothing");

    let mut passes = 0;
    let report = h.step(|ui| {
        ui.request_relayout();
        passes += 1;
    });
    assert_eq!(passes, 2, "the last pass has no third");
    assert!(report.repaint_requested, "its ask takes the next frame");
    assert!(
        !h.step(|_| {}).repaint_requested,
        "the next frame asked none"
    );

    let asking = |ui: &mut Ui| ui.request_relayout();
    for frame in 1..=MAX_CARRIED_RELAYOUTS {
        assert!(h.step(asking).repaint_requested, "frame {frame}");
    }
    assert!(!h.step(|_| {}).repaint_requested);
    for frame in 1..=MAX_CARRIED_RELAYOUTS {
        assert!(h.step(asking).repaint_requested, "again, frame {frame}");
    }
    let log = LoopLog::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer({
            let log = log.clone();
            move || log.clone()
        })
        .finish();
    subscriber::with_default(subscriber, || {
        assert!(!h.step(asking).repaint_requested, "the loop stops");
        assert!(!h.step(asking).repaint_requested, "and stays stopped");
    });
    let logged = String::from_utf8(log.0.lock_unpoisoned().clone()).unwrap();
    assert_eq!(logged.matches("ERROR").count(), 1, "one error: {logged}");
    assert!(logged.contains("a relayout loop"), "{logged}");
    assert!(!h.step(|_| {}).repaint_requested);
    assert!(h.step(asking).repaint_requested, "the count starts again");
}

/// A relayout request forces a second record pass; `frame_value` returns pass A's value, which observes one-frame edges.
#[test]
fn frame_value_records_both_relayout_passes_and_returns_the_first() {
    let mut h = Harness::mono(SURFACE);
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

/// `WindowFrames::frame` plumbs `now`, `dt` (clamped to `MAX_ANIMATION_DT`) and the repaint-requested flag (reset every call) end-to-end to `FrameOutput`.
#[test]
fn frame_plumbs_now_dt_and_repaint_request() {
    let mut h = Harness::mono(UVec2::new(100, 100));
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
    assert_eq!(h.ui().frame_runtime.stamp().time, Duration::from_millis(16));
    assert_eq!(
        h.ui().frame_runtime.dt,
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
            ui.frame_runtime.request_repaint();
        })
        .repaint_requested;
    assert!(
        repaint,
        "repaint_requested set during recording must surface on FrameOutput",
    );
    assert_eq!(h.ui().frame_runtime.stamp().time, Duration::from_millis(32));
    assert_eq!(
        h.ui().frame_runtime.dt,
        Duration::from_millis(16).as_secs_f32(),
        "FrameRuntime::dt should be next-frame delta",
    );

    // Frame C: a 5s gap clamps dt to MAX_ANIMATION_DT; `time` still tracks the true clock.
    let _ = h.at(Duration::from_millis(5_032)).frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    assert_eq!(
        h.ui().frame_runtime.stamp().time,
        Duration::from_millis(5_032)
    );
    assert_eq!(
        h.ui().frame_runtime.dt,
        MAX_ANIMATION_DT,
        "FrameRuntime::dt should clamp at MAX_ANIMATION_DT",
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
    use crate::ui::app::App;
    use crate::window::window_token::WindowToken;

    #[derive(Debug)]
    struct Probe {
        id: WidgetId,
        seen: Vec<bool>,
    }

    impl App for Probe {
        fn update(&mut self, _win: WindowToken, ctx: UpdateCtx<'_>) -> InputOutcome {
            let response = ctx.input.response_for(self.id);
            self.seen.push(response.pressed() && response.pointer_over);
            InputOutcome::Record
        }

        fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
            Button::new().id(self.id).size(40.0).show(ui);
        }
    }

    let mut h = Harness::mono(SURFACE);
    let mut probe = Probe {
        id: WidgetId::from_hash("update-reads-press"),
        seen: Vec::new(),
    };
    h.frame_app(&mut probe);
    assert_eq!(probe.seen, [false], "nothing pressed yet");

    h.press(Vec2::new(20.0, 20.0), PointerButton::Left);
    h.frame_app(&mut probe);
    assert_eq!(probe.seen, [false, true], "the press reaches update");
}

#[derive(Debug, Default)]
struct CountingApp {
    updates: u32,
    records: u32,
    relayout_on_next_record: bool,
    expected_pointer: Option<Vec2>,
}

impl App for CountingApp {
    fn update(&mut self, win: WindowToken, ctx: UpdateCtx<'_>) -> InputOutcome {
        assert_eq!(win, WindowToken(0));
        assert_eq!(ctx.display().physical, SURFACE);
        assert_eq!(
            self.records,
            self.updates * 2,
            "update runs before any record of its frame"
        );
        assert_eq!(
            ctx.record_frame_id(),
            u64::from(self.updates),
            "one id per recorded frame"
        );
        assert_eq!(ctx.input.peek_pointer_pos(), self.expected_pointer);
        self.updates += 1;
        InputOutcome::Record
    }

    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
        assert_eq!(win, WindowToken(0));
        self.records += 1;
        if self.relayout_on_next_record {
            self.relayout_on_next_record = false;
            ui.request_relayout();
        }
    }
}

/// `App::update` runs once per recorded frame and `App::record` once per pass: twice
/// on the cold first frame and on a relayout, never on a paint-only frame.
#[test]
fn app_lifecycle_follows_frame_plan_and_record_replays() {
    let mut h = Harness::cold(SURFACE);
    let mut app = CountingApp::default();
    let pointer = Vec2::new(24.0, 12.0);
    h.hover(pointer);
    app.expected_pointer = Some(pointer);

    let processing = h.frame_app(&mut app).processing;
    assert_eq!(processing, FrameProcessing::SingleLayout);
    assert_eq!(app.updates, 1, "cold-start frame updates once");
    assert_eq!(app.records, 2, "cold-start warmup and pass A both record");

    app.relayout_on_next_record = true;
    h.ui_mut().request_repaint();
    let processing = h
        .at(Duration::from_millis(16))
        .frame_app(&mut app)
        .processing;
    assert_eq!(processing, FrameProcessing::DoubleLayout);
    assert_eq!(app.updates, 2, "relayout frame adds one update");
    assert_eq!(app.records, 4, "relayout frame records pass A and pass B");

    h.ui_mut()
        .frame_runtime
        .schedule_wake(Duration::from_millis(32), WakeReasons::ANIMATION, None);
    let processing = h
        .at(Duration::from_millis(32))
        .frame_app(&mut app)
        .processing;
    assert_eq!(processing, FrameProcessing::PaintOnly);
    assert_eq!(app.updates, 2, "paint-only frame skips update");
    assert_eq!(app.records, 4, "paint-only frame skips record");
}

fn probe_button(ui: &mut Ui, id: WidgetId) {
    Button::new().id(id).size(40.0).show(ui);
}

/// One frame's reads of a widget's click, the focus and a key.
#[derive(Clone, Copy, Debug, PartialEq)]
struct UpdateReads {
    clicked: bool,
    focus: Option<WidgetId>,
    escape: bool,
}

/// Holds every frame's update and `update` to what its first record pass reads.
#[derive(Debug)]
struct UpdateProbe {
    first: WidgetId,
    second: WidgetId,
    updated: Option<WidgetId>,
    read: Option<UpdateReads>,
    clicks: u32,
    escapes: u32,
    last_focus: Option<WidgetId>,
}

impl App for UpdateProbe {
    fn update(&mut self, _: WindowToken, mut ctx: UpdateCtx<'_>) -> InputOutcome {
        self.updated = ctx.input.focus();
        self.read = Some(UpdateReads {
            clicked: ctx.input.response_for(self.first).left.clicked(),
            focus: ctx.input.focus(),
            escape: ctx.key_pressed(Key::Escape),
        });
        InputOutcome::Record
    }

    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        let reads = UpdateReads {
            clicked: ui.response_for(self.first).left.clicked(),
            focus: ui.focus(),
            escape: ui.key_pressed(Key::Escape),
        };
        if let Some(stage) = self.read.take() {
            assert_eq!(stage, reads, "the update and pass A read one frame");
            assert_eq!(self.updated, reads.focus, "update reads that focus too");
            self.clicks += u32::from(reads.clicked);
            self.escapes += u32::from(reads.escape);
            self.last_focus = reads.focus;
        }
        let (first, second) = (self.first, self.second);
        Panel::hstack().id_salt("stage.row").show(ui, |ui| {
            probe_button(ui, first);
            probe_button(ui, second);
        });
    }
}

/// The update, `update` and pass A read one frame: a click's edge, a key's edge, and focus
/// already moved by this frame's Tab. Tab from `first` lands on `second`.
#[test]
fn the_update_reads_what_the_first_pass_reads() {
    let first = WidgetId::from_hash("stage.first");
    let second = WidgetId::from_hash("stage.second");
    let mut probe = UpdateProbe {
        first,
        second,
        updated: None,
        read: None,
        clicks: 0,
        escapes: 0,
        last_focus: None,
    };
    let mut h = Harness::mono(SURFACE);
    for _ in 0..2 {
        h.frame_app(&mut probe);
    }

    h.click(Vec2::new(10.0, 10.0));
    h.frame_app(&mut probe);
    assert_eq!(probe.clicks, 1);

    h.key(Key::Escape);
    h.frame_app(&mut probe);
    assert_eq!(probe.escapes, 1);

    h.ui_mut().set_focus(first);
    h.frame_app(&mut probe);
    assert_eq!(probe.last_focus, Some(first));
    h.key(Key::Tab);
    h.frame_app(&mut probe);
    assert_eq!(probe.last_focus, Some(second));
}

/// Asks for a second pass each frame, with a key read in the update or none.
#[derive(Debug)]
struct WatchProbe {
    reads_key: bool,
}

impl App for WatchProbe {
    fn update(&mut self, _: WindowToken, mut ctx: UpdateCtx<'_>) -> InputOutcome {
        if self.reads_key {
            let _ = ctx.key_pressed(Key::Escape);
        }
        InputOutcome::Record
    }

    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        ui.request_relayout();
    }
}

/// A key the update reads stays watched past a second pass, whose reset would drop it: with
/// nothing focused, only a watched press asks for a frame.
#[test]
fn a_watch_from_the_update_outlives_a_second_pass() {
    let asks_a_frame = |reads_key: bool| {
        let mut h = Harness::mono(SURFACE);
        let mut app = WatchProbe { reads_key };
        h.frame_app(&mut app);
        h.ui_mut().request_repaint();
        let report = h.frame_app(&mut app);
        assert_eq!(report.processing, FrameProcessing::DoubleLayout);
        h.on_input(InputEvent::key_down(Key::Escape))
            .repaint_requested
    };
    assert!(asks_a_frame(true));
    assert!(!asks_a_frame(false));
}

/// Records a button with a state row, and reads both in each update.
#[derive(Debug, Default)]
struct WarmProbe {
    reads: Vec<(Option<Rect>, Option<u32>)>,
}

impl App for WarmProbe {
    fn update(&mut self, _: WindowToken, ctx: UpdateCtx<'_>) -> InputOutcome {
        let id = WidgetId::from_hash("warm");
        self.reads.push((
            ctx.input.response_for(id).rect,
            ctx.state::<u32>(id).copied(),
        ));
        InputOutcome::Record
    }

    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        let id = WidgetId::from_hash("warm");
        probe_button(ui, id);
        ui.set_state(id, 7_u32);
    }
}

/// The first update runs ahead of the warmup, as an app sets up in it, so it reads no rect and no
/// state row, as no frame is on screen. The next reads what the first frame left: 40 x 40 at the
/// origin, and 7.
#[test]
fn the_first_update_runs_ahead_of_the_warmup() {
    let mut h = Harness::cold(SURFACE);
    let mut app = WarmProbe::default();
    h.frame_app(&mut app);
    h.ui_mut().request_repaint();
    h.frame_app(&mut app);
    assert_eq!(
        app.reads,
        [
            (None, None),
            (Some(Rect::new(0.0, 0.0, 40.0, 40.0)), Some(7)),
        ],
    );
}

/// Reads focus in each update and each record pass, over two Tab stops.
#[derive(Debug, Default)]
struct FocusProbe {
    in_update: Vec<Option<WidgetId>>,
    in_record: Vec<Option<WidgetId>>,
}

impl App for FocusProbe {
    fn update(&mut self, _: WindowToken, ctx: UpdateCtx<'_>) -> InputOutcome {
        self.in_update.push(ctx.input.focus());
        InputOutcome::Record
    }

    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        self.in_record.push(ui.focus());
        for salt in ["stop-a", "stop-b"] {
            Block::new()
                .id(WidgetId::from_hash(salt))
                .size(40.0)
                .focusable(true)
                .show(ui);
        }
    }
}

/// A Tab before the window opens counts only over a focus set by then: it moves that focus from
/// `a` to `b` in pass A's routing, past the first update and the warmup, which read `a`. With no
/// focus it is dropped as it arrives, so no pass reads a move.
#[test]
fn a_tab_before_the_first_frame_moves_a_set_focus_in_its_first_pass() {
    let (a, b) = (WidgetId::from_hash("stop-a"), WidgetId::from_hash("stop-b"));
    let mut h = Harness::cold(SURFACE);
    let mut app = FocusProbe::default();
    h.ui_mut().set_focus(a);
    h.key(Key::Tab);
    h.frame_app(&mut app);
    assert_eq!(app.in_update[0], Some(a));
    assert_eq!(app.in_record[..2], [Some(a), Some(b)]);

    let mut h = Harness::cold(SURFACE);
    let mut app = FocusProbe::default();
    h.key(Key::Tab);
    h.frame_app(&mut app);
    assert_eq!(app.in_update[0], None);
    assert_eq!(app.in_record[..2], [None, None]);
}

/// Writes a state row in the update, and reads it in each record pass.
#[derive(Debug, Default)]
struct StateProbe {
    written: u32,
    times: Vec<Duration>,
    line_heights: Vec<f32>,
    read: Vec<Option<u32>>,
}

impl App for StateProbe {
    fn update(&mut self, _: WindowToken, mut ctx: UpdateCtx<'_>) -> InputOutcome {
        self.written += 1;
        self.times.push(ctx.now());
        self.line_heights.push(ctx.theme().text.line_height());
        ctx.set_state(WidgetId::from_hash("row"), self.written);
        InputOutcome::Record
    }

    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        let id = WidgetId::from_hash("row");
        self.read.push(ui.state::<u32>(id).copied());
        probe_button(ui, id);
    }
}

/// A state the update writes shows in that frame's first record pass.
#[test]
fn a_state_the_update_writes_shows_in_the_first_pass() {
    let mut h = Harness::mono(SURFACE);
    let mut app = StateProbe::default();
    h.frame_app(&mut app);
    h.ui_mut().request_repaint();
    h.at(Duration::from_millis(16)).frame_app(&mut app);
    assert_eq!(app.read, [Some(1), Some(2)]);
    assert_eq!(app.times, [Duration::ZERO, Duration::from_millis(16)]);
    let shown = h.ui().theme().text.line_height();
    assert_eq!(app.line_heights, [shown, shown]);
}

/// A paint-only frame repaints the retained tree, so a move of what that tree was built under
/// records instead: a theme set between frames (its colors are in the tree), or a font another
/// window loaded into the shared shaper (its text is). Before, a caret blink repainted the old
/// colors and text until input arrived.
#[test]
fn a_theme_or_font_move_records_where_an_animation_frame_would_only_paint() {
    use crate::text::font_scope::internals::INTER;
    use crate::ui::resources::UiResources;

    let resources = UiResources::isolated_text();
    let mut h = Harness::from_resources(resources.clone(), SURFACE);
    let mut other = Harness::from_resources(resources, SURFACE);
    let record = |ui: &mut Ui| {
        Button::new()
            .id(WidgetId::from_hash("b"))
            .label("label")
            .show(ui);
    };
    h.prime(2, record);
    other.prime(2, record);
    let mut ms = 16;
    let mut animation_frame = |h: &mut Harness| {
        ms += 16;
        let at = Duration::from_millis(ms);
        h.ui_mut()
            .frame_runtime
            .schedule_wake(at, WakeReasons::ANIMATION, None);
        h.at(at).frame(record).processing
    };
    assert_eq!(
        animation_frame(&mut h),
        FrameProcessing::PaintOnly,
        "premise"
    );

    h.ui_mut()
        .set_theme(ColorScheme::Dark, support::still_theme(ColorScheme::Dark));
    assert_ne!(
        animation_frame(&mut h),
        FrameProcessing::PaintOnly,
        "a set theme"
    );
    assert_eq!(
        animation_frame(&mut h),
        FrameProcessing::PaintOnly,
        "recorded once, it paints again"
    );

    other
        .ui()
        .load_font(INTER)
        .expect("the bundled Inter loads");
    assert_ne!(
        animation_frame(&mut h),
        FrameProcessing::PaintOnly,
        "a font another window loaded"
    );
    assert_eq!(animation_frame(&mut h), FrameProcessing::PaintOnly);
}
