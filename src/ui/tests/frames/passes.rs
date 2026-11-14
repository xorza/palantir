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
use crate::ui::app::{App, RecordPass};
use crate::ui::frame_report::FrameProcessing;
use crate::ui::frame_runtime::MAX_CARRIED_RELAYOUTS;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::tests::support::{self, SURFACE};
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
                pass_a_rect.set(resp.rect);
                ui.request_relayout();
            }
            1 => {
                // Pass B: `response_for` returns pass A's arranged rect.
                pass_b_rect.set(resp.rect);
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
        let render_frame_before = h.ui().frame_runtime.render_frame_idx;
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
            h.ui().frame_runtime.render_frame_idx,
            render_frame_before + 1,
            "{label}: render_frame_idx must bump once per frame (passes: {expected})",
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

/// Records one button, and notes what each pass read of its click.
#[derive(Debug, Default)]
struct ClickProbe {
    reads: Vec<(RecordPass, bool)>,
}

impl ClickProbe {
    const BUTTON: &str = "click.button";
}

impl App for ClickProbe {
    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        let clicked = Button::new()
            .id(WidgetId::from_hash(Self::BUTTON))
            .label("Go")
            .size((100.0, 40.0))
            .show(ui)
            .left
            .clicked();
        self.reads.push((ui.record_pass(), clicked));
    }
}

/// A click shows in the first pass alone and records again, and a frame with no edge records once.
#[test]
fn a_click_reads_in_the_first_pass_and_the_second_has_no_edge() {
    let button = WidgetId::from_hash(ClickProbe::BUTTON);
    let mut h = Harness::mono(SURFACE);
    let mut app = ClickProbe::default();
    h.frame_app(&mut app);
    h.frame_app(&mut app);
    h.press(button, PointerButton::Left);
    h.frame_app(&mut app);
    assert!(h.ui().response_for(button).left.held(), "the capture holds");
    h.release(PointerButton::Left);

    app.reads.clear();
    let report = h.frame_app(&mut app);
    assert_eq!(
        app.reads,
        [(RecordPass::First, true), (RecordPass::Second, false)],
    );
    assert_eq!(report.processing, FrameProcessing::DoubleLayout);

    app.reads.clear();
    h.ui_mut().request_repaint();
    let report = h.frame_app(&mut app);
    assert_eq!(app.reads, [(RecordPass::First, false)]);
    assert_eq!(report.processing, FrameProcessing::SingleLayout);
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

/// `WindowFrames::frame` plumbs `now`, `dt` (clamped to `MAX_ANIMATION_DT`) and the
/// repaint-requested flag (reset every call) end-to-end to `FrameReport`.
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

    // Frame B: an unsettled animation tick during recording must reach `FrameReport`.
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

/// A first pass reads a response ahead of any widget, so its quiescence snapshot must be this
/// frame's: a stale one from a pass with the pointer off the surface would default the interaction
/// half out.
#[test]
fn a_first_pass_sees_input_that_arrived_after_a_quiescent_pass() {
    #[derive(Debug)]
    struct Probe {
        id: WidgetId,
        seen: Vec<bool>,
    }

    impl App for Probe {
        fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
            let response = ui.response_for(self.id);
            self.seen
                .push(response.is_pressed() && response.pointer_over);
            Button::new().id(self.id).size(40.0).show(ui);
        }
    }

    let mut h = Harness::mono(SURFACE);
    let mut probe = Probe {
        id: WidgetId::from_hash("first-pass-reads-press"),
        seen: Vec::new(),
    };
    h.frame_app(&mut probe);
    assert_eq!(probe.seen, [false], "nothing pressed yet");

    h.press(Vec2::new(20.0, 20.0), PointerButton::Left);
    h.frame_app(&mut probe);
    assert_eq!(
        probe.seen,
        [false, true],
        "the press reaches the first pass"
    );
}

#[derive(Debug, Default)]
struct CountingApp {
    passes: Vec<RecordPass>,
    frames_begun: u64,
    relayout_on_next_record: bool,
    expected_pointer: Option<Vec2>,
}

impl App for CountingApp {
    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
        assert_eq!(win, WindowToken(0));
        assert_eq!(ui.display().physical, SURFACE);
        let pass = ui.record_pass();
        self.passes.push(pass);
        match pass {
            RecordPass::Warmup => assert_eq!(ui.peek_pointer_pos(), None, "a scratch input"),
            RecordPass::First => {
                assert_eq!(ui.peek_pointer_pos(), self.expected_pointer);
                assert_eq!(ui.record_frame_idx(), self.frames_begun, "one id per frame");
                self.frames_begun += 1;
            }
            RecordPass::Second => {
                assert_eq!(
                    ui.record_frame_idx() + 1,
                    self.frames_begun,
                    "the first's id"
                );
            }
        }
        if self.relayout_on_next_record {
            self.relayout_on_next_record = false;
            ui.request_relayout();
        }
    }
}

/// `App::record` runs once per pass, and each pass names itself: the warmup and the first on a cold
/// frame, the first and the second on a relayout, and none on a paint-only frame.
#[test]
fn app_lifecycle_follows_frame_plan_and_each_pass_names_itself() {
    let mut h = Harness::cold(SURFACE);
    let mut app = CountingApp::default();
    let pointer = Vec2::new(24.0, 12.0);
    h.hover(pointer);
    app.expected_pointer = Some(pointer);

    let processing = h.frame_app(&mut app).processing;
    assert_eq!(processing, FrameProcessing::SingleLayout);
    assert_eq!(app.passes, [RecordPass::Warmup, RecordPass::First]);

    app.passes.clear();
    app.relayout_on_next_record = true;
    h.ui_mut().request_repaint();
    let processing = h
        .at(Duration::from_millis(16))
        .frame_app(&mut app)
        .processing;
    assert_eq!(processing, FrameProcessing::DoubleLayout);
    assert_eq!(app.passes, [RecordPass::First, RecordPass::Second]);

    app.passes.clear();
    h.ui_mut()
        .frame_runtime
        .schedule_wake(Duration::from_millis(32), WakeReasons::ANIMATION, None);
    let processing = h
        .at(Duration::from_millis(32))
        .frame_app(&mut app)
        .processing;
    assert_eq!(processing, FrameProcessing::PaintOnly);
    assert_eq!(app.passes, [], "paint-only frame skips record");
}

fn probe_button(ui: &mut Ui, id: WidgetId) {
    Button::new().id(id).size(40.0).show(ui);
}

/// Reads a click, a key and the focus at the top of each pass, ahead of the widgets.
#[derive(Debug)]
struct TopReads {
    first: WidgetId,
    second: WidgetId,
    clicks: u32,
    escapes: u32,
    last_focus: Option<WidgetId>,
}

impl App for TopReads {
    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        self.clicks += u32::from(ui.response_for(self.first).left.clicked());
        self.escapes += u32::from(ui.key_pressed(Key::Escape));
        if ui.record_pass() == RecordPass::First {
            self.last_focus = ui.focus();
        }
        let (first, second) = (self.first, self.second);
        Panel::hstack().id_salt("stage.row").show(ui, |ui| {
            probe_button(ui, first);
            probe_button(ui, second);
        });
    }
}

/// The top of a first pass reads one click's edge and one key's, once over the frame's passes, and
/// focus already moved by this frame's Tab. Tab from `first` lands on `second`.
#[test]
fn a_first_pass_reads_a_click_a_key_and_the_moved_focus() {
    let first = WidgetId::from_hash("stage.first");
    let second = WidgetId::from_hash("stage.second");
    let mut probe = TopReads {
        first,
        second,
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

/// Asks for a second pass each frame, with a key read in the first pass or none.
#[derive(Debug)]
struct WatchProbe {
    reads_key: bool,
}

impl App for WatchProbe {
    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        if self.reads_key && ui.record_pass() == RecordPass::First {
            let _ = ui.key_pressed(Key::Escape);
        }
        ui.request_relayout();
    }
}

/// A key the first pass reads stays watched past a second pass, as an app reads its input in the
/// first alone: with nothing focused, only a watched press asks for a frame. The next frame's first
/// pass starts the watches again, so a read it leaves out no longer wakes.
#[test]
fn a_watch_from_the_first_pass_outlives_a_second_pass() {
    let asks_a_frame = |reads_key: bool, then_reads: bool| {
        let mut h = Harness::mono(SURFACE);
        let mut app = WatchProbe { reads_key };
        h.frame_app(&mut app);
        h.ui_mut().request_repaint();
        let report = h.frame_app(&mut app);
        assert_eq!(report.processing, FrameProcessing::DoubleLayout);
        app.reads_key = then_reads;
        h.ui_mut().request_repaint();
        h.frame_app(&mut app);
        h.on_input(InputEvent::key_down(Key::Escape))
            .repaint_requested
    };
    assert!(asks_a_frame(true, true));
    assert!(!asks_a_frame(true, false));
    assert!(!asks_a_frame(false, false));
}

/// Reads a button's rect and its state row at the top of each pass, and writes the row.
#[derive(Debug, Default)]
struct WarmProbe {
    reads: Vec<(RecordPass, Option<Rect>, Option<u32>)>,
}

impl App for WarmProbe {
    fn record(&mut self, _: WindowToken, ui: &mut Ui) {
        let id = WidgetId::from_hash("warm");
        self.reads.push((
            ui.record_pass(),
            ui.response_for(id).rect,
            ui.state::<u32>(id).copied(),
        ));
        probe_button(ui, id);
        ui.set_state(id, 7_u32);
    }
}

/// The warmup runs first, with no frame on screen, so it reads no rect and no state row. The first
/// pass past it reads what the warmup left: 40 x 40 at the origin, and 7.
#[test]
fn the_first_pass_reads_what_the_warmup_left() {
    let mut h = Harness::cold(SURFACE);
    let mut app = WarmProbe::default();
    h.frame_app(&mut app);
    assert_eq!(
        app.reads,
        [
            (RecordPass::Warmup, None, None),
            (
                RecordPass::First,
                Some(Rect::new(0.0, 0.0, 40.0, 40.0)),
                Some(7),
            ),
        ],
    );
}

/// Reads focus in each record pass, over two Tab stops.
#[derive(Debug, Default)]
struct FocusProbe {
    in_record: Vec<Option<WidgetId>>,
}

impl App for FocusProbe {
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
/// `a` to `b` in pass A's routing, past the warmup, which reads `a`. With no focus it is dropped as
/// it arrives, so no pass reads a move.
#[test]
fn a_tab_before_the_first_frame_moves_a_set_focus_in_its_first_pass() {
    let (a, b) = (WidgetId::from_hash("stop-a"), WidgetId::from_hash("stop-b"));
    let mut h = Harness::cold(SURFACE);
    let mut app = FocusProbe::default();
    h.ui_mut().set_focus(a);
    h.key(Key::Tab);
    h.frame_app(&mut app);
    assert_eq!(app.in_record[..2], [Some(a), Some(b)]);

    let mut h = Harness::cold(SURFACE);
    let mut app = FocusProbe::default();
    h.key(Key::Tab);
    h.frame_app(&mut app);
    assert_eq!(app.in_record[..2], [None, None]);
}

/// A paint-only frame repaints the retained tree, so a move of what that tree was built under
/// records instead: a theme set between frames (its colors are in the tree), or a font another
/// window loaded into the shared shaper (its text is), so a caret blink never repaints the old
/// colors and text.
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
        .set_scheme_theme(ColorScheme::Dark, support::still_theme(ColorScheme::Dark))
        .unwrap();
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
