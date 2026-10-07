//! Window requests a frame queues, and the close veto's one-frame life.

use crate::display::user_scale::UserScale;
use crate::internals::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::window::window_commands::WindowCommands;
use crate::window::window_placement::WindowPlacement;
use crate::window::window_token::WindowToken;
use glam::{IVec2, UVec2};

/// `open_window` / `close_window` requests must survive the `frame` call that filed them, since the host drains after.
#[test]
fn window_requests_queue_and_survive_the_frame() {
    use crate::WindowConfig;

    let mut h = UiHarness::new(SURFACE);
    let open = WindowToken(7);
    let close = WindowToken(3);

    h.frame(|ui| {
        ui.open_window(open, WindowConfig::new("inspector"));
        ui.close_window(close);
    });

    // Filed during record, still pending after the frame returned.
    assert_eq!(h.ui.window_requests.commands.opens.len(), 1);
    assert_eq!(h.ui.window_requests.commands.opens[0].token, open);
    assert_eq!(
        h.ui.window_requests.commands.opens[0].config.title,
        "inspector"
    );
    assert_eq!(h.ui.window_requests.commands.closes, vec![close]);

    // A quiet frame must not drop the still-undrained queue.
    h.frame(|_| {});
    assert_eq!(
        h.ui.window_requests.commands.opens.len(),
        1,
        "queue must outlive a quiet frame"
    );
    assert_eq!(h.ui.window_requests.commands.closes, vec![close]);

    // Emulate the host draining by `append`/`drain`; a third frame leaves them empty.
    h.ui.window_requests.commands.opens.clear();
    h.ui.window_requests.commands.closes.clear();
    h.frame(|_| {});
    assert!(h.ui.window_requests.commands.opens.is_empty());
    assert!(h.ui.window_requests.commands.closes.is_empty());

    // `is_window_open` polls the host-refreshed live set, not the pending queues.
    assert!(!h.ui.is_window_open(open), "empty live set ⇒ nothing open");
    h.ui.window_directory().add(open);
    assert!(h.ui.is_window_open(open));
    assert!(!h.ui.is_window_open(close), "only `open` is live");

    let placed = WindowPlacement {
        position: Some(IVec2::new(-120, 48)),
        maximized: true,
    };
    h.ui.window_frame.placement = placed;
    let geometry = h.ui.window_geometry();
    assert_eq!(geometry.inner_size, SURFACE);
    assert_eq!(geometry.placement, placed);
    assert_eq!(
        WindowConfig::new("restored")
            .with_placement(geometry.placement)
            .placement,
        placed,
    );
}

/// The OS-close veto protocol: [`Ui::close_requested`] reflects the host's `wants_close`, [`Ui::keep_open`] sets the veto, and `WindowRequests::drain` resolves them.
#[test]
fn close_request_veto_protocol() {
    let mut h = UiHarness::new(SURFACE);

    assert!(
        !h.frame_value(|ui| ui.close_requested()),
        "no close pending ⇒ close_requested() false"
    );
    assert!(!h.ui.window_requests.close_vetoed);

    // Host signals a close; an app that vetoes keeps the window open.
    h.ui.window_frame.close_requested = true;
    h.ui.window_requests.close_vetoed = false;
    let requested = h.frame_value(|ui| {
        ui.keep_open();
        ui.close_requested()
    });
    assert!(requested, "host signalled close ⇒ close_requested() true");
    assert!(
        h.ui.window_requests.close_vetoed,
        "keep_open must set the veto the host reads"
    );
    let me = WindowToken(0);
    let mut out = WindowCommands::default();
    h.ui.window_requests
        .drain(me, h.ui.window_frame.close_requested, &mut out);
    assert!(
        out.closes.is_empty(),
        "a vetoed request must NOT resolve to a close"
    );

    // Same signal, app ignores it: a real close (the drain above spent the veto).
    assert!(h.frame_value(|ui| ui.close_requested()));
    assert!(!h.ui.window_requests.close_vetoed, "untouched ⇒ no veto");
    h.ui.window_requests
        .drain(me, h.ui.window_frame.close_requested, &mut out);
    assert_eq!(
        out.closes,
        [me],
        "an un-vetoed request must resolve to a close"
    );
}

/// Record passes replay, so one `open_window` call reaches the queue several times per frame: dedup by token, last config wins.
#[test]
fn open_window_dedups_by_token_within_a_frame() {
    use crate::window::window_config::WindowConfig;
    let mut h = UiHarness::new(SURFACE);
    let cfg = WindowConfig::new;
    h.ui.open_window(WindowToken(7), cfg("first"));
    h.ui.open_window(WindowToken(7), cfg("second"));
    h.ui.open_window(WindowToken(8), cfg("other"));
    assert_eq!(h.ui.window_requests.commands.opens.len(), 2);
    assert_eq!(h.ui.window_requests.commands.opens[0].token, WindowToken(7));
    assert_eq!(
        h.ui.window_requests.commands.opens[0].config.title,
        "second"
    );
    assert_eq!(h.ui.window_requests.commands.opens[1].token, WindowToken(8));
}

/// `window_geometry` answers in the window manager's logical pixels, so a persisted geometry reopens the same window whatever the user scale: a 200×200 surface at dpr 2 is 100×100 to the platform at any scale, while dividing by the product would shrink it 20% per launch at 125%.
#[test]
fn window_geometry_reports_the_platform_space_not_the_ui_space() {
    let mut h = UiHarness::new(SURFACE).scale(2.0);
    h.frame(|_| {});
    assert_eq!(h.ui.window_geometry().inner_size, UVec2::new(100, 100));

    let mut zoomed = UiHarness::new(SURFACE)
        .scale(2.0)
        .user_scale(UserScale::new(1.25).unwrap());
    zoomed.frame(|_| {});
    assert_eq!(
        zoomed.ui.display.logical_size().w,
        80.0,
        "the UI is laid out in the scaled space",
    );
    assert_eq!(
        zoomed.ui.window_geometry().inner_size,
        UVec2::new(100, 100),
        "and the platform hears the unscaled one",
    );
}

/// IME is a level asked for each pass, like the cursor; a non-offset caret panics where passed.
#[test]
fn request_ime_is_a_per_pass_level() {
    use crate::internals::panic_probe;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::math::domain;

    let caret = Rect::new(10.0, 20.0, 2.0, 16.0);
    let mut h = UiHarness::new(SURFACE);
    let report = h.frame(|ui| ui.request_ime(caret));
    assert_eq!(report.ime_area, Some(caret));
    let report = h.frame(|_| {});
    assert_eq!(
        report.ime_area, None,
        "a pass that does not ask turns it off"
    );

    panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| ui.request_ime(Rect::new(f32::NAN, 0.0, 2.0, 16.0)));
    });
}
