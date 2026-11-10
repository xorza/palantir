use crate::display::system_scale::SystemScale;
use crate::driver::action::Action;
use crate::driver::agent_window::AgentWindow;
use crate::driver::error::{LocateError, PlayError, StepError};
use crate::driver::locator::{Found, Locator};
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::test_gpu::{
    headless_test_gpu, isolated_headless_test_gpu, isolated_headless_test_gpu_capped,
};
use crate::host::harness::harness_builder::Depth;
use crate::host::harness::{Harness, device_answered};
use crate::internals::panic_probe::assert_panics_with;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::ui::Ui;
use crate::ui::app::App;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::widgets::text_edit::TextEdit;
use crate::window::window_commands::WindowCommand;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use half::f16;
use std::cell::Cell;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

fn tile(ui: &mut Ui) {
    Block::new()
        .id_salt("tile")
        .size(20.0)
        .chrome(Chrome::new(RgbaF32::WHITE))
        .show(ui);
}

/// A record frame builds no draw list; a compose frame builds the scene's, here the one tile at
/// the origin, physical as logical at scale 1. A second still frame of either plans no paint and
/// asks no repaint, so it builds nothing: the buffer, emptied, stays empty.
#[test]
fn each_depth_builds_what_it_names() {
    let tile_rect = Rect::new(0.0, 0.0, 20.0, 20.0);
    for (depth, built) in [(Depth::Record, vec![]), (Depth::Compose, vec![tile_rect])] {
        let mut h = Harness::builder(UVec2::new(64, 32))
            .mono()
            .depth(depth)
            .build();
        let quads = |h: &Harness| -> Vec<Rect> {
            h.render_buffer()
                .quads
                .instance()
                .iter()
                .map(|quad| quad.header.rect)
                .collect()
        };
        let first = h.frame(tile);
        assert_eq!(first.paint(), FramePaint::Full);
        assert_eq!(quads(&h), built);

        let still = h.frame(tile);
        assert_eq!(still.paint(), FramePaint::Skip);
        assert!(!still.repaint_requested);
        assert_eq!(quads(&h), built, "a still frame builds nothing new");
    }
}

/// A frame that opens and closes windows records each command, in order, and runs on.
#[test]
fn a_frame_that_opens_a_window_records_the_command() {
    let mut h = Harness::mono(UVec2::new(64, 32));
    let mut opened = false;
    h.frame(|ui: &mut Ui| {
        if !opened {
            ui.open_window(WindowToken(3), WindowConfig::new("Inspector"));
            opened = true;
        }
    });
    h.frame(|ui: &mut Ui| ui.close_window(WindowToken(3)));
    let commands = h.window_commands();
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert!(
        matches!(&commands[0], WindowCommand::Open { token: WindowToken(3), config } if config.title == "Inspector")
    );
    assert!(matches!(commands[1], WindowCommand::Close(WindowToken(3))));
    // An app that opens the window each frame, as no window but the harness's opens here, records
    // one open. Each close records, as the harness's own window closes, and closes again once an
    // app launched again in it quits, with no open between.
    for _ in 0..3 {
        h.frame(|ui: &mut Ui| ui.open_window(WindowToken(3), WindowConfig::new("Inspector")));
    }
    h.frame(|ui: &mut Ui| ui.close_window(WindowToken(0)));
    h.frame(|ui: &mut Ui| ui.close_window(WindowToken(0)));
    let commands = h.window_commands();
    assert_eq!(commands.len(), 5, "{commands:?}");
    assert!(matches!(
        commands[2],
        WindowCommand::Open {
            token: WindowToken(3),
            ..
        }
    ));
    assert!(matches!(commands[3], WindowCommand::Close(WindowToken(0))));
    assert!(matches!(commands[4], WindowCommand::Close(WindowToken(0))));
}

/// The app a person plays: buttons 80 × 24 on a canvas, "One" at (10, 10) and two "Twin" at
/// (10, 40) and (10, 70), a text edit at (10, 100), and "Hidden" placed below a 40 × 20 clip at
/// (200, 10). It counts what each button saw and keeps what the editor committed.
#[derive(Debug, Default)]
struct Board {
    one: u32,
    twin_b: u32,
    double: u32,
    text: String,
    committed: Option<String>,
}

fn edit() -> WidgetId {
    WidgetId::from_hash("edit")
}

fn button(ui: &mut Ui, id: &str, label: &str, pos: Vec2) -> Response {
    Button::new()
        .id(WidgetId::from_hash(id))
        .label(label)
        .size((Sizing::fixed(80.0), Sizing::fixed(24.0)))
        .at(pos)
        .show(ui)
}

impl App for Board {
    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let one = button(ui, "one", "One", Vec2::new(10.0, 10.0));
                if one.clicked() {
                    self.one += 1;
                }
                if one.double_clicked() {
                    self.double += 1;
                }
                Button::new()
                    .id(WidgetId::from_hash("off"))
                    .label("Off")
                    .size((Sizing::fixed(80.0), Sizing::fixed(24.0)))
                    .at(Vec2::new(200.0, 40.0))
                    .disabled(true)
                    .show(ui);
                button(ui, "twin-a", "Twin", Vec2::new(10.0, 40.0));
                if button(ui, "twin-b", "Twin", Vec2::new(10.0, 70.0)).clicked() {
                    self.twin_b += 1;
                }
                let edit = TextEdit::new(&mut self.text)
                    .id(edit())
                    .size((Sizing::fixed(120.0), Sizing::fixed(24.0)))
                    .at(Vec2::new(10.0, 100.0))
                    .show(ui);
                if edit.committed {
                    self.committed = Some(self.text.clone());
                }
                Panel::canvas()
                    .auto_id()
                    .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                    .at(Vec2::new(200.0, 10.0))
                    .clip_rect()
                    .show(ui, |ui| {
                        button(ui, "hidden", "Hidden", Vec2::new(0.0, 50.0));
                    });
            });
    }
}

/// A person plays the board on a painting window: a click by text clicks once; "Twin" names two
/// and refuses, `Nth` picks the second; a clipped button is out of view; a disabled one refuses a
/// click and takes a hover; a drag 2 px long, under the 4 px threshold, refuses; two clicks are
/// two clicks and a double click one double click; typed text commits on Enter. A failed step
/// names its index.
#[test]
fn play_acts_as_a_person() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(400, 200))
        .depth(Depth::gpu(gpu.handles()))
        .build();
    let mut app = Board::default();
    h.session(&mut app).settle().unwrap();

    h.session(&mut app).play(&[Action::click("One")]).unwrap();
    assert_eq!((app.one, app.double), (1, 0));

    let twin = Locator::from("Twin");
    let Err(StepError {
        step: 0,
        error: PlayError::Locate(LocateError::Ambiguous { matches, .. }),
    }) = h.session(&mut app).play(&[Action::click(twin.clone())])
    else {
        panic!("two buttons show \"Twin\"");
    };
    assert_eq!(matches.len(), 2);
    h.session(&mut app)
        .play(&[Action::click(twin.nth(1))])
        .unwrap();
    assert_eq!(app.twin_b, 1);

    assert!(matches!(
        h.session(&mut app)
            .play(&[Action::Hover("One".into()), Action::click("Hidden")]),
        Err(StepError {
            step: 1,
            error: PlayError::Locate(LocateError::OffScreen { .. }),
        })
    ));

    assert!(matches!(
        h.session(&mut app)
            .play(&[Action::Hover("Off".into()), Action::click("Off")]),
        Err(StepError {
            step: 1,
            error: PlayError::Locate(LocateError::Disabled { .. }),
        })
    ));
    let short = Action::<Infallible>::Drag {
        from: "One".into(),
        to: Locator::from("One").offset(Vec2::new(2.0, 0.0)),
        button: PointerButton::Left,
        mods: ShortcutMods::NONE,
        steps: Action::DRAG_STEPS,
    };
    assert_eq!(
        h.session(&mut app).play(&[short]),
        Err(StepError {
            step: 0,
            error: PlayError::ShortDrag { travel: 2.0 },
        })
    );
    assert_eq!(app.one, 1, "the refused drag clicked nothing");

    h.session(&mut app)
        .play(&[Action::click("One"), Action::click("One")])
        .unwrap();
    assert_eq!((app.one, app.double), (3, 0), "two clicks, spaced apart");
    h.session(&mut app)
        .play(&[Action::<Infallible>::DoubleClick("One".into())])
        .unwrap();
    assert_eq!(app.double, 1, "one double click");

    h.session(&mut app)
        .play(&[
            Action::click(edit()),
            Action::Type("hello".to_owned()),
            Action::InsertText(", world".to_owned()),
            Action::Key(Shortcut::key(Key::Enter)),
        ])
        .unwrap();
    assert_eq!(app.committed.as_deref(), Some("hello, world"));
}

/// A `KeyDown` step leaves its key held across steps and lets its modifiers go; a `KeyUp` step
/// releases the key.
#[test]
fn a_key_down_step_holds_its_key_until_a_key_up_step() {
    let mut h = Harness::mono(UVec2::new(64, 32));
    let space = Key::Char(' ');
    let held = Cell::new(false);
    let mut app = |ui: &mut Ui| held.set(ui.key_held(space));
    h.session(&mut app)
        .play(&[Action::<Infallible>::KeyDown(Shortcut::new(
            ShortcutMods::SHIFT,
            space,
        ))])
        .unwrap();
    assert!(held.get());
    assert_eq!(h.ui().input().modifiers_after_held_input(), Modifiers::NONE);
    h.session(&mut app)
        .play(&[Action::<Infallible>::KeyUp(space.into())])
        .unwrap();
    assert!(!held.get());
}

/// Each step settles: a `Wait` runs frames of one frame's time until it passed. A step with no
/// modifiers leaves held what is held, and a click with modifiers releases them, so no modifier
/// reads as held after it.
#[test]
fn a_step_settles_and_lets_go() {
    let mut h = Harness::mono(UVec2::new(400, 200));
    let mut app = Board::default();
    h.session(&mut app).settle().unwrap();
    let before = h.time;
    h.session(&mut app)
        .play(&[Action::<Infallible>::Wait(Duration::from_millis(100))])
        .unwrap();
    assert!(h.time >= before + Duration::from_millis(100));
    h.hold_modifiers(Modifiers::ALT);
    h.session(&mut app)
        .play(&[
            Action::<Infallible>::Key(Shortcut::key(Key::Escape)),
            Action::<Infallible>::Hover("One".into()),
        ])
        .unwrap();
    assert_eq!(h.ui().input().modifiers_after_held_input(), Modifiers::ALT);
    h.session(&mut app)
        .play(&[Action::<Infallible>::Click {
            at: "One".into(),
            button: PointerButton::Left,
            mods: ShortcutMods::CTRL | ShortcutMods::SHIFT,
        }])
        .unwrap();
    assert_eq!(app.one, 1);
    assert_eq!(h.ui().input().modifiers_after_held_input(), Modifiers::NONE);
}

/// An app that asks a repaint each frame never settles: the settle stops at the builder's bound,
/// 600 frames.
#[test]
fn an_app_that_always_repaints_is_unsettled() {
    let mut h = Harness::mono(UVec2::new(64, 32));
    let mut app = |ui: &mut Ui| ui.request_repaint();
    assert_eq!(
        h.session(&mut app).settle(),
        Err(PlayError::Unsettled { frames: 600 })
    );
}

/// An app busy with work on another thread, which wakes the window and ends the work when done.
#[derive(Debug)]
struct Worker {
    busy: Arc<AtomicBool>,
    frames: u32,
}

impl App for Worker {
    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        self.frames += 1;
        tile(ui);
    }

    fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

/// Busy work that steps once a frame and wakes the window for the next step.
#[derive(Debug)]
struct Stepping {
    steps_left: u32,
    frames: u32,
}

impl App for Stepping {
    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        self.frames += 1;
        tile(ui);
        if self.steps_left > 0 {
            self.steps_left -= 1;
            ui.waker().wake();
        }
    }

    fn is_busy(&self) -> bool {
        self.steps_left > 0
    }
}

/// A settle waits out a busy app: a worker ends its work and wakes the window after the settle
/// began, and the settle runs the woken frame and returns. A wake waits until a frame takes it.
/// A busy app that never wakes the window fails the settle once the builder's background wait
/// passes. Work that wakes the window as it goes holds the settle, and its woken frames count
/// against no frame bound, but the settle holds for twenty background waits at most.
#[test]
fn a_settle_waits_for_a_busy_apps_wake() {
    let mut h = Harness::builder(UVec2::new(64, 32))
        .mono()
        .background_wait(Duration::MAX)
        .build();
    let busy = Arc::new(AtomicBool::new(true));
    let mut app = Worker {
        busy: Arc::clone(&busy),
        frames: 0,
    };
    h.frame_app(&mut app);
    let waker = h.ui().waker();
    let work = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        busy.store(false, Ordering::Release);
        waker.wake();
    });
    let before = app.frames;
    h.session(&mut app).settle().unwrap();
    work.join().unwrap();
    assert!(!app.is_busy());
    assert!(app.frames > before, "the woken frame recorded");
    assert!(!h.has_pending_wake(), "the settle's frame took the wake");
    h.ui().waker().wake();
    assert!(h.has_pending_wake());
    h.frame_app(&mut app);
    assert!(!h.has_pending_wake());

    let mut stuck = Harness::builder(UVec2::new(64, 32))
        .mono()
        .background_wait(Duration::from_millis(10))
        .build();
    let mut app = Worker {
        busy: Arc::new(AtomicBool::new(true)),
        frames: 0,
    };
    assert_eq!(
        stuck.session(&mut app).settle(),
        Err(PlayError::Busy {
            waited: Duration::from_millis(10)
        })
    );

    // Each frame of the work wakes the window for the next, past the 3 frames that bound a
    // settle: frames 1 to 10 step it, the 10th wake brings frame 11, which finds it done.
    let mut progressing = Harness::builder(UVec2::new(64, 32))
        .mono()
        .settle_frames(3)
        .build();
    let mut app = Stepping {
        steps_left: 10,
        frames: 0,
    };
    assert_eq!(progressing.session(&mut app).settle(), Ok(11));
    assert_eq!((app.steps_left, app.frames), (0, 11));

    // Work that never ends but wakes the window each frame holds a settle for twenty background
    // waits, 20 ms at 1 ms, then fails it.
    let mut endless = Harness::builder(UVec2::new(64, 32))
        .mono()
        .background_wait(Duration::from_millis(1))
        .build();
    let mut app = Stepping {
        steps_left: u32::MAX,
        frames: 0,
    };
    match endless.session(&mut app).settle() {
        Err(PlayError::StillBusy { waited }) => {
            assert!(waited > Duration::from_millis(20), "{waited:?}");
        }
        other => panic!("an endless busy app holds the settle to its bound: {other:?}"),
    }
}

/// A session loses the harness's device, which a harness below `Depth::Gpu` has none of. On a lost
/// device a shot, and a settle that waits on a screenshot, report the loss: no copy comes back,
/// where both ran out their frames and read as an animation that runs on. A new device recovers
/// the harness, whose next shot is the window at its size again.
#[test]
fn a_lost_device_fails_a_shot_and_a_settle_on_one() {
    let mut app = tile;
    let mut unpainted = Harness::builder(UVec2::new(64, 32)).build();
    assert_eq!(
        unpainted.session(&mut app).lose_device(),
        Err(PlayError::NoPixels)
    );

    let gpu = isolated_headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(64, 32))
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.session(&mut app).settle().unwrap();
    assert!(!gpu.is_lost());
    h.session(&mut app).lose_device().unwrap();
    assert!(gpu.is_lost());
    assert_eq!(h.session(&mut app).shot(None), Err(PlayError::DeviceLost));
    assert_eq!(h.wait_for_gpu(), GpuWait::DeviceLost);
    h.ui_mut().request_screenshot(None).unwrap();
    assert_eq!(h.session(&mut app).settle(), Err(PlayError::DeviceLost));
    assert!(h.is_device_lost());

    let fresh = isolated_headless_test_gpu();
    h.recover(fresh.handles());
    assert!(!h.is_device_lost());
    h.session(&mut app).settle().unwrap();
    let shot = h
        .session(&mut app)
        .shot(None)
        .expect("the recovered harness paints");
    assert_eq!(shot.image().size(), UVec2::new(64, 32));
}

/// A recovery keeps the window's size, so a window wider than the new device's largest texture
/// panics: 2049 px on a device capped at 2048, the floor Palantir runs on.
#[test]
fn a_recovery_onto_a_device_too_small_for_the_window_panics() {
    let mut h = Harness::builder(UVec2::new(2049, 1))
        .depth(Depth::gpu(headless_test_gpu().handles()))
        .build();
    let small = isolated_headless_test_gpu_capped(2048);
    assert_panics_with(
        "a target's size is 1..=2048 physical pixels per side, got [2049, 1]",
        || h.recover(small.handles()),
    );
}

/// A shot at scale 1.5 of a 400 × 200 logical window is 600 × 300 texels; a shot of the logical
/// region (10, 10, 20, 10) covers its physical pixels, 15..45 × 15..30: 30 × 15. A window that
/// paints nothing has no shot, and a region outside the window holds no pixel.
#[test]
fn a_shot_is_the_painted_window() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(600, 300))
        .system_scale(SystemScale::new(1.5).unwrap())
        .depth(Depth::gpu(gpu.handles()))
        .build();
    let mut app = Board::default();
    h.session(&mut app).settle().unwrap();
    let whole = h.session(&mut app).shot(None).unwrap();
    assert_eq!(whole.image().size(), UVec2::new(600, 300));
    assert_eq!(whole.scale(), 1.5);
    let region = h
        .session(&mut app)
        .shot(Some(Rect::new(10.0, 10.0, 20.0, 10.0)))
        .unwrap();
    assert_eq!(region.image().size(), UVec2::new(30, 15));
    assert_eq!(
        h.session(&mut app)
            .shot(Some(Rect::new(900.0, 900.0, 5.0, 5.0))),
        Err(PlayError::EmptyRegion)
    );

    let mut cpu = Harness::mono(UVec2::new(64, 32));
    assert_eq!(cpu.session(&mut app).shot(None), Err(PlayError::NoPixels));
}

/// A capture runs one frame at the parked clock and moves it no further, and its bytes are the
/// pixels a shot of the same still window reads, the next capture's too. Below the GPU it has
/// none.
#[test]
fn a_capture_is_one_frame_at_the_parked_clock() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(120, 80))
        .system_scale(SystemScale::new(2.0).unwrap())
        .depth(Depth::gpu(gpu.handles()))
        .build();
    let mut app = Board::default();
    h.session(&mut app).settle().unwrap();
    let at = Duration::from_secs(7);
    h.at(at);
    let frames = h.ui().render_frame_id();
    let mut pixels = Vec::new();
    h.capture(&mut app, &mut pixels).unwrap();
    assert_eq!(h.ui().render_frame_id(), frames + 1, "one frame");
    assert_eq!(h.time, at, "the clock stays");
    assert_eq!(pixels.len(), 120 * 80 * 4);
    let shot = h.session(&mut app).shot(None).unwrap();
    assert_eq!(pixels, shot.image().pixels);
    let mut again = Vec::new();
    h.capture(&mut app, &mut again).unwrap();
    assert_eq!(
        again, pixels,
        "a second copy through the kept staging buffer"
    );

    let mut cpu = Harness::mono(UVec2::new(64, 32));
    assert_eq!(cpu.capture(&mut app, &mut pixels), Err(PlayError::NoPixels));
}

/// A `Rgba16Float` window captures its linear values as half floats, 8 bytes a pixel: a block
/// filled `(0.25, 0.5, 1, 1)` over a black ground reads back those values exactly, each a power
/// of two an f16 holds.
#[test]
fn a_float_window_captures_its_linear_values() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(16, 8))
        .depth(Depth::gpu_float(gpu.handles()))
        .build();
    h.ui_mut().theme_mut().window_bg = RgbaF32::BLACK;
    let mut app = |ui: &mut Ui| {
        Block::new()
            .id_salt("fill")
            .size(8.0)
            .chrome(Chrome::new(RgbaF32::new(0.25, 0.5, 1.0, 1.0)))
            .show(ui);
    };
    let mut pixels = Vec::new();
    h.capture(&mut app, &mut pixels).unwrap();
    assert_eq!(pixels.len(), 16 * 8 * 8);
    let texel = |x: usize, y: usize| {
        let at = (y * 16 + x) * 8;
        let lanes: [u8; 8] = pixels[at..at + 8].try_into().unwrap();
        bytemuck::cast::<[u8; 8], [u16; 4]>(lanes).map(|bits| f16::from_bits(bits).to_f32())
    };
    assert_eq!(texel(2, 2), [0.25, 0.5, 1.0, 1.0], "the block");
    assert_eq!(texel(12, 2), [0.0, 0.0, 0.0, 1.0], "the ground");
}

/// The input methods aim by text, by id and by point alike, and `widgets` lists what a locator
/// reads, by text, ignoring case, the clipped one left out.
#[test]
fn input_aims_by_text_id_and_point() {
    let mut h = Harness::new(UVec2::new(400, 200));
    let mut app = Board::default();
    h.prime(2, |ui: &mut Ui| app.record(WindowToken(0), ui));
    h.click("One");
    h.frame(|ui: &mut Ui| app.record(WindowToken(0), ui));
    h.advance_past_double_click();
    h.click(WidgetId::from_hash("one"));
    h.frame(|ui: &mut Ui| app.record(WindowToken(0), ui));
    h.advance_past_double_click();
    h.click(Vec2::new(20.0, 20.0));
    h.frame(|ui: &mut Ui| app.record(WindowToken(0), ui));
    assert_eq!(app.one, 3);

    let mut window = h.session(&mut app);
    let texts: Vec<String> = window
        .widgets(Some("twin"))
        .unwrap()
        .into_iter()
        .map(|widget| widget.text)
        .collect();
    assert_eq!(texts, ["Twin", "Twin"]);
    assert!(window.widgets(Some("hidden")).unwrap().is_empty());
}

/// A read hands back the app and the last frame, and runs no frame and moves no clock, as a find
/// through it does, which also says when a locator names nothing; the next caret blink only paints,
/// so `record_frame_id` holds. A query changes the app and asks a repaint, so the frame at the same
/// blink records: `record_frame_id` moves by exactly 1. A settle over a blink's frame alone runs
/// that frame and returns 0.
#[test]
fn a_session_reads_with_no_frame_and_queries_with_one() {
    let blink = Duration::from_secs(1);
    let mut h = Harness::builder(UVec2::new(320, 200)).build();
    let mut app = Board::default();
    h.session(&mut app).settle().unwrap();
    h.session(&mut app).play(&[Action::click(edit())]).unwrap();
    let recorded = h.ui().record_frame_id();
    let (frames, time) = (h.ui().render_frame_id(), h.time);

    let read = h
        .session(&mut app)
        .read(|app, ui| (app.text.clone(), ui.record_frame_id()));
    assert_eq!(read, Ok((String::new(), recorded)));
    let field = h.rect(edit()).unwrap();
    assert_eq!(
        h.session(&mut app)
            .find(&Locator::<Infallible>::from(edit())),
        Ok(Found::center_of(field))
    );
    assert!(matches!(
        h.session(&mut app)
            .find(&Locator::<Infallible>::from("Missing")),
        Err(PlayError::Locate(LocateError::NotFound { .. }))
    ));
    assert_eq!(h.ui().render_frame_id(), frames, "a read runs no frame");
    assert_eq!(h.time, time, "a read moves no clock");
    h.advance(blink);
    h.frame_app(&mut app);
    assert_eq!(h.ui().record_frame_id(), recorded, "the blink only paints");

    assert_eq!(
        h.session(&mut app)
            .query(|app, _| app.text = "typed".to_owned()),
        Ok(())
    );
    assert_eq!(app.text, "typed");
    assert_eq!(
        h.ui().render_frame_id(),
        frames + 1,
        "a query runs no frame by itself"
    );
    h.advance(blink);
    h.frame_app(&mut app);
    assert_eq!(
        h.ui().record_frame_id(),
        recorded + 1,
        "the frame after a query records"
    );

    let rendered = h.ui().render_frame_id();
    h.advance(blink);
    assert_eq!(
        h.session(&mut app).settle(),
        Ok(0),
        "a settle counts the frames recorded, and the blink's frame only paints"
    );
    assert_eq!(h.ui().render_frame_id(), rendered + 1);

    // A `Wait` of 100 ms is 6 frames of 1/60 s, and its settle one more: 7 run. No blink falls in
    // them, so each records, and `play` counts all 7.
    let (recorded, rendered) = (h.ui().record_frame_id(), h.ui().render_frame_id());
    let played = h
        .session(&mut app)
        .play(&[Action::<Infallible>::Wait(Duration::from_millis(100))]);
    assert_eq!(h.ui().render_frame_id(), rendered + 7);
    assert_eq!(played, Ok(7));
    assert_eq!(h.ui().record_frame_id(), recorded + 7);
}

/// A white 2 × 1 logical block at the origin over a black clear. At scale 1 on 8 × 6 it covers
/// physical x 0..2, y 0..1; moved to 4 × 4 at scale 2 it covers x 0..4, y 0..2. Each read-back is
/// the frame just drawn, at the window's size.
#[test]
fn a_new_display_reads_back_the_new_size_and_scale() {
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(UVec2::new(8, 6))
        .mono()
        .depth(Depth::gpu(gpu.handles()))
        .build();
    h.ui_mut().theme_mut().window_bg = RgbaF32::BLACK;
    let app = |ui: &mut Ui| {
        Block::new()
            .id_salt("white")
            .size((2.0, 1.0))
            .chrome(Chrome::new(RgbaF32::WHITE))
            .show(ui);
    };
    let white = |x0: u32, x1: u32, y0: u32, y1: u32| {
        move |x: u32, y: u32| (x0..x1).contains(&x) && (y0..y1).contains(&y)
    };
    for (size, scale, inside) in [
        (UVec2::new(8, 6), 1.0, white(0, 2, 0, 1)),
        (UVec2::new(4, 4), 2.0, white(0, 4, 0, 2)),
    ] {
        h.resize(size)
            .set_system_scale(SystemScale::new(scale).unwrap());
        h.frame(app);
        let image = h.read_back();
        assert_eq!(image.dimensions(), (size.x, size.y), "{size}");
        for (x, y, pixel) in image.enumerate_pixels() {
            let want = if inside(x, y) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(pixel.0, want, "{size} at ({x}, {y})");
        }
    }
}

/// Only a wait that finished lets a step go on, and each other names its own cause.
#[test]
fn a_device_wait_reads_as_its_own_case() {
    assert_eq!(device_answered(GpuWait::Done), Ok(()));
    assert_eq!(
        device_answered(GpuWait::DeviceLost),
        Err(PlayError::DeviceLost)
    );
    assert_eq!(
        device_answered(GpuWait::TimedOut),
        Err(PlayError::GpuTimedOut)
    );
}
