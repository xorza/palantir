//! Window requests a frame queues, and the close veto's one-frame life.

use crate::ui::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::window::window_placement::WindowPlacement;
use glam::IVec2;

/// `open_window` / `close_window` enqueue onto the retained scratch the
/// host drains *after* the frame — so the requests must survive the
/// `frame` call that filed them (and a subsequent quiet frame), since
/// the host hasn't had a chance to run yet. Without that, a window
/// opened during record would be silently dropped before the event loop
/// regained `&ActiveEventLoop` to act on it.
#[test]
fn window_requests_queue_and_survive_the_frame() {
    use crate::{WindowConfig, WindowToken};

    let mut h = UiHarness::new(SURFACE);
    let open = WindowToken(7);
    let close = WindowToken(3);

    h.frame(|ui| {
        ui.open_window(open, WindowConfig::new("inspector"));
        ui.close_window(close);
    });

    // Filed during record, still pending after the frame returned —
    // nothing in the frame pipeline clears them.
    assert_eq!(h.ui.window_requests.commands.opens.len(), 1);
    assert_eq!(h.ui.window_requests.commands.opens[0].token, open);
    assert_eq!(
        h.ui.window_requests.commands.opens[0].config.title,
        "inspector"
    );
    assert_eq!(h.ui.window_requests.commands.closes, vec![close]);

    // A quiet frame (no new requests) must not drop the still-undrained
    // queue — the host might not have ticked between these two frames.
    h.frame(|_| {});
    assert_eq!(
        h.ui.window_requests.commands.opens.len(),
        1,
        "queue must outlive a quiet frame"
    );
    assert_eq!(h.ui.window_requests.commands.closes, vec![close]);

    // The host drains by `append`/`drain`-ing the vecs; emulate that and
    // confirm a third frame leaves them empty (no re-queue).
    h.ui.window_requests.commands.opens.clear();
    h.ui.window_requests.commands.closes.clear();
    h.frame(|_| {});
    assert!(h.ui.window_requests.commands.opens.is_empty());
    assert!(h.ui.window_requests.commands.closes.is_empty());

    // `window_open` polls the host-refreshed live set (here set directly,
    // as the host would before each frame) — not the pending queues.
    assert!(!h.ui.window_open(open), "empty live set ⇒ nothing open");
    h.ui.resources.windows.set_live(open, true);
    assert!(h.ui.window_open(open));
    assert!(!h.ui.window_open(close), "only `open` is live");

    // The placement travels whole, so what the app persists is what a
    // `WindowConfig` takes back — no field-by-field translation between
    // the host's facts, the geometry and the config.
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
            .placement(geometry.placement)
            .placement,
        placed,
    );
}

/// The OS-close veto protocol between the host and app code:
/// [`Ui::close_requested`] reflects the host's per-frame `wants_close`
/// signal, and [`Ui::keep_open`] sets the veto the host reads back to
/// decide whether to actually close. The host's decision rule is
/// `wants_close && !close_vetoed` (the tail of `WinitHost::draw`); pin it
/// here so the two flags can't drift out from under that resolution.
#[test]
fn close_request_veto_protocol() {
    let mut h = UiHarness::new(SURFACE);

    // No close pending: the flag is false and keep_open never fires.
    h.frame(|ui| {
        assert!(
            !ui.close_requested(),
            "no close pending ⇒ close_requested() false"
        );
    });
    assert!(!h.ui.window_requests.close_vetoed);

    // Host signals a close; an app that vetoes keeps the window open.
    h.ui.window_frame.close_requested = true;
    h.ui.window_requests.close_vetoed = false;
    h.frame(|ui| {
        assert!(
            ui.close_requested(),
            "host signalled close ⇒ close_requested() true"
        );
        ui.keep_open();
    });
    assert!(
        h.ui.window_requests.close_vetoed,
        "keep_open must set the veto the host reads"
    );
    let should_close = h.ui.window_frame.close_requested && !h.ui.window_requests.close_vetoed;
    assert!(
        !should_close,
        "a vetoed request must NOT resolve to a close"
    );

    // Same signal, app ignores it: resolves to a real close. (The host
    // resets the veto before every draw.)
    h.ui.window_requests.close_vetoed = false;
    h.frame(|ui| {
        assert!(ui.close_requested());
    });
    assert!(!h.ui.window_requests.close_vetoed, "untouched ⇒ no veto");
    let should_close = h.ui.window_frame.close_requested && !h.ui.window_requests.close_vetoed;
    assert!(should_close, "an un-vetoed request must resolve to a close");
}

/// Record passes replay (cold-start warmup, double-layout pass B), so
/// one logical `open_window` call reaches the queue two or three times
/// per frame — dedup by token, last config wins.
#[test]
fn open_window_dedups_by_token_within_a_frame() {
    use crate::window::window_config::WindowConfig;
    use crate::window::window_token::WindowToken;
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
