//! Window requests a frame queues, and the close veto's one-frame life.

use crate::display::user_scale::UserScale;
use crate::host::harness::Harness;
use crate::ui::tests::support::SURFACE;
use crate::window::window_commands::WindowCommand;
use crate::window::window_placement::WindowPlacement;
use crate::window::window_token::WindowToken;
use glam::{IVec2, UVec2};

/// `open_window` / `close_window` requests reach the host after the frame that filed them, once:
/// the harness drains them as a host does, and a quiet frame files nothing more.
#[test]
fn window_requests_reach_the_host_after_the_frame() {
    use crate::window::window_config::WindowConfig;

    let mut h = Harness::mono(SURFACE);
    let open = WindowToken(7);
    let close = WindowToken(3);

    h.frame(|ui| {
        ui.open_window(open, WindowConfig::new("inspector"));
        ui.close_window(close);
    });
    let seen = |h: &Harness| -> Vec<String> {
        h.window_commands()
            .iter()
            .map(|command| match command {
                WindowCommand::Open { token, config } => format!("open {token:?} {}", config.title),
                WindowCommand::Close(token) => format!("close {token:?}"),
            })
            .collect()
    };
    let filed = ["open WindowToken(7) inspector", "close WindowToken(3)"];
    assert_eq!(seen(&h), filed);
    assert!(h.ui().window_requests.commands.seen().is_empty(), "drained");

    h.frame(|_| {});
    assert_eq!(seen(&h), filed, "a quiet frame files nothing");

    // `is_window_open` polls the host-refreshed live set, not the pending queues.
    assert!(
        !h.ui().is_window_open(open),
        "empty live set ⇒ nothing open"
    );
    h.ui().window_directory().add(open);
    assert!(h.ui().is_window_open(open));
    assert!(!h.ui().is_window_open(close), "only `open` is live");

    let placed = WindowPlacement {
        physical_pos: Some(IVec2::new(-120, 48)),
        maximized: true,
    };
    h.ui().window_frame.placement = placed;
    let geometry = h.ui().window_geometry();
    assert_eq!(geometry.inner_size, SURFACE);
    assert_eq!(geometry.placement, placed);
    assert_eq!(
        WindowConfig::new("restored")
            .with_placement(geometry.placement)
            .placement,
        placed,
    );
}

/// The OS-close veto protocol: [`Ui::close_requested`] reflects the host's `wants_close`,
/// [`Ui::keep_open`] sets the veto, and the host's drain after the frame resolves them: a vetoed
/// request closes nothing, an ignored one closes the window.
#[test]
fn close_request_veto_protocol() {
    let mut h = Harness::mono(SURFACE);

    assert!(
        !h.frame_value(|ui| ui.close_requested()),
        "no close pending ⇒ close_requested() false"
    );

    // Host signals a close; an app that vetoes keeps the window open.
    h.ui().window_frame.close_requested = true;
    let requested = h.frame_value(|ui| {
        ui.keep_open();
        ui.close_requested()
    });
    assert!(requested, "host signaled close ⇒ close_requested() true");
    assert!(
        h.window_commands().is_empty(),
        "a vetoed request must NOT resolve to a close"
    );
    assert!(
        !h.ui().window_requests.close_vetoed,
        "the drain spent the veto"
    );

    // Same signal, app ignores it: a real close.
    h.ui().window_frame.close_requested = true;
    assert!(h.frame_value(|ui| ui.close_requested()));
    assert!(
        matches!(h.window_commands(), [WindowCommand::Close(WindowToken(0))]),
        "an un-vetoed request must resolve to a close"
    );
}

/// `window_geometry` answers in the window manager's logical pixels, so a persisted geometry reopens the same window whatever the user scale: a 200×200 surface at dpr 2 is 100×100 to the platform at any scale, while dividing by the product would shrink it 20% per launch at 125%.
#[test]
fn window_geometry_reports_the_platform_space_not_the_ui_space() {
    let mut h = Harness::builder(SURFACE).mono().scale(2.0).build();
    h.frame(|_| {});
    assert_eq!(h.ui().window_geometry().inner_size, UVec2::new(100, 100));

    let mut zoomed = Harness::builder(SURFACE)
        .mono()
        .scale(2.0)
        .user_scale(UserScale::new(1.25).unwrap())
        .build();
    zoomed.frame(|_| {});
    assert_eq!(
        zoomed.ui().display().logical_size().w,
        80.0,
        "the UI is laid out in the scaled space",
    );
    assert_eq!(
        zoomed.ui().window_geometry().inner_size,
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
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        assert_eq!(ui.ime_area(), None, "each pass starts with it off");
        ui.request_ime(caret);
        assert_eq!(ui.ime_area(), Some(caret));
    });
    assert_eq!(h.ui().ime_area(), Some(caret), "it outlives the frame");
    h.frame(|_| {});
    assert_eq!(
        h.ui().ime_area(),
        None,
        "a pass that does not ask turns it off"
    );

    panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
        let mut h = Harness::mono(SURFACE);
        h.frame(|ui| ui.request_ime(Rect::new(f32::NAN, 0.0, 2.0, 16.0)));
    });
}
