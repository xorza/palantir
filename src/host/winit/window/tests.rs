use std::time::{Duration, Instant};

use glam::Vec2;

use crate::host::window_driver::WindowDriver;
use crate::host::winit::input::PointerTrace;
use crate::host::winit::window::{FramePresent, PointerAnchor};

use crate::ui::resources::UiResources;
use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;

const AT: Vec2 = Vec2::new(300.0, 120.0);

fn anchor(scale: f32) -> Option<PointerAnchor> {
    PointerAnchor::after(None, PointerTrace::At(AT), scale)
}

#[test]
fn a_trace_records_a_position_and_a_departure_clears_one() {
    let held = anchor(2.0);
    assert_eq!(held.map(|a| a.physical), Some(AT));

    assert_eq!(
        PointerAnchor::after(held, PointerTrace::Unchanged, 4.0),
        held,
        "an event carrying no position leaves the anchor alone, scale included",
    );
    assert_eq!(PointerAnchor::after(held, PointerTrace::Gone, 2.0), None);
    assert_eq!(
        PointerAnchor::after(None, PointerTrace::Unchanged, 2.0),
        None,
    );
}

/// The pointer sits at physical (300, 120). At scale 2 the recorder
/// holds (150, 60); after a move to 2.5 it must hold (120, 48).
#[test]
fn a_scale_move_restates_the_position_once() {
    let mut held = anchor(2.0).unwrap();
    assert_eq!(held.restate_at(2.5), Some(Vec2::new(120.0, 48.0)));
    assert_eq!(
        held.restate_at(2.5),
        None,
        "the anchor adopted the scale, so the recorder is up to date",
    );
    assert_eq!(
        held.restate_at(2.0),
        Some(Vec2::new(150.0, 60.0)),
        "and a move back restates the position it started at",
    );
}

#[test]
fn an_unmoved_scale_restates_nothing() {
    assert_eq!(anchor(1.5).unwrap().restate_at(1.5), None);
}

#[test]
fn frame_drain_collects_commands_and_applies_close_veto() {
    let shared = UiResources::isolated_mono();
    let token = WindowToken(17);
    let mut driver = WindowDriver::builder(token, &shared, true).build();
    let opened = WindowToken(18);
    let mut commands = WindowCommands::default();

    driver
        .ui
        .open_window(opened, WindowConfig::new("inspector"));
    driver.ui.set_cursor(CursorIcon::Pointer);
    driver.ui.window_frame_mut().close_requested = true;

    let output = driver.drain_window_output(&mut commands);
    assert_eq!(output.cursor, CursorIcon::Pointer);
    assert_eq!(
        output.vsync,
        Vsync::On,
        "a frame that asked for nothing reports the standing level"
    );
    assert_eq!(commands.opens.len(), 1);
    assert_eq!(commands.opens[0].token, opened);
    assert_eq!(
        commands.closes,
        [token],
        "an un-vetoed close becomes this window's own close command"
    );
    assert!(driver.ui.window_requests().commands.opens.is_empty());
    assert!(driver.ui.window_requests().commands.closes.is_empty());
    // Drained by `append`, not `mem::take`, so the recorder keeps its
    // buffers for the next frame instead of reallocating per window
    // command.
    let open_capacity = driver.ui.window_requests().commands.opens.capacity();
    let close_capacity = driver.ui.window_requests().commands.closes.capacity();
    assert!(open_capacity > 0 && close_capacity > 0);

    driver.ui.window_frame_mut().close_requested = true;
    driver.ui.keep_open();
    let mut vetoed = WindowCommands::default();
    driver.drain_window_output(&mut vetoed);
    assert!(vetoed.closes.is_empty());

    // A second drain after the veto must not resurrect the request: the
    // frame state was consumed, so nothing is pending.
    let mut settled = WindowCommands::default();
    driver.drain_window_output(&mut settled);
    assert!(settled.closes.is_empty());
    assert!(!driver.ui.window_requests().close_vetoed);
    assert_eq!(
        driver.ui.window_requests().commands.opens.capacity(),
        open_capacity,
        "draining must not hand away the recorder's buffer"
    );
    assert_eq!(
        driver.ui.window_requests().commands.closes.capacity(),
        close_capacity
    );
}

/// Vsync is a level like the cursor, not a one-shot request: the drain
/// copies it, it survives the drain that delivered it, and it reads back
/// through `Ui::vsync` so an app never mirrors it. Collapsing a repeated
/// level into no swapchain work is the host's job, not the recorder's —
/// see `Window::set_vsync`.
#[test]
fn vsync_is_a_level_the_drain_copies_and_the_recorder_keeps() {
    let shared = UiResources::isolated_mono();
    let mut driver = WindowDriver::builder(WindowToken(3), &shared, true).build();
    let mut commands = WindowCommands::default();

    assert_eq!(driver.ui.vsync(), Vsync::On, "vsync is on unless asked off");
    assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::On);

    driver.ui.set_vsync(Vsync::Off);
    assert_eq!(driver.ui.vsync(), Vsync::Off, "the setter reads back");
    assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::Off);
    assert_eq!(
        driver.drain_window_output(&mut commands).vsync,
        Vsync::Off,
        "the level survives the drain that delivered it",
    );

    // Within one pass the last writer wins, matching `set_cursor`.
    driver.ui.set_vsync(Vsync::On);
    driver.ui.set_vsync(Vsync::Off);
    assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::Off);
}

#[test]
fn due_deadlines_resolve_to_immediate_and_future_ones_stand() {
    let now = Instant::now();
    let past = now - Duration::from_millis(1);
    let future = now + Duration::from_millis(16);

    assert_eq!(FramePresent::At(past).resolve(now), FramePresent::Immediate);
    // `<=` — a deadline landing exactly on `now` is due, not pending.
    assert_eq!(FramePresent::At(now).resolve(now), FramePresent::Immediate);
    assert_eq!(
        FramePresent::At(future).resolve(now),
        FramePresent::At(future)
    );
    assert_eq!(
        FramePresent::Immediate.resolve(now),
        FramePresent::Immediate
    );
    assert_eq!(FramePresent::Idle.resolve(now), FramePresent::Idle);
}
