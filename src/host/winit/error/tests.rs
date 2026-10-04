use std::error::Error;

use crate::gpu::error::{GpuRequestError, SurfaceError, UnmetRequirements};
use crate::host::winit::error::{HostDisconnected, WinitHostError};
use crate::window::window_token::WindowToken;
use winit::error;

#[test]
fn host_errors_preserve_sources_and_explain_capability_failures() {
    let event_loop = WinitHostError::CreateEventLoop {
        source: error::EventLoopError::RecreationAttempt,
    };
    assert_eq!(
        event_loop.to_string(),
        "failed to create event loop: EventLoop can't be recreated"
    );
    assert!(event_loop.source().is_some());

    // A device request fails while a window opens its surface, so it
    // arrives as that window's surface error. The surface forwards the
    // request's words and hands on *its* cause rather than itself, so a
    // chain printer never meets the same sentence twice.
    let unmet = UnmetRequirements::Limit {
        name: "max_immediate_size",
        required: 16,
        available: 8,
    };
    let capability = WinitHostError::Surface {
        token: WindowToken(7),
        source: SurfaceError::Device {
            source: GpuRequestError::Requirements {
                source: unmet.clone(),
            },
        },
    };
    let request = "the graphics adapter cannot run Palantir: \
         graphics device limit max_immediate_size is 8, but Palantir requires 16";
    assert_eq!(
        capability.to_string(),
        format!("window WindowToken(7) surface failed: {request}"),
    );
    let surface = capability.source().expect("the surface error");
    assert_eq!(surface.to_string(), request);
    assert_eq!(
        surface.source().map(ToString::to_string),
        Some(unmet.to_string()),
        "the chain skips the device wrapper, which added no words of its own",
    );
}

#[test]
fn host_disconnected_reports_the_loss_and_costs_nothing_to_return() {
    // `run_on_main` returns this by value on a path the caller reaches
    // during shutdown; a payload would be dead weight, since there is
    // no `&mut T` left to re-run the closure against.
    assert_eq!(size_of::<HostDisconnected>(), 0);

    // The message has to name the consequence, not just the cause —
    // "event loop exited" alone reads as routine shutdown, while the
    // point is that submitted work was thrown away.
    let err = HostDisconnected;
    assert_eq!(
        err.to_string(),
        "host event loop has exited; the scheduled work was not delivered"
    );
    assert!(err.source().is_none());

    // Usable through `?` into a boxed error, which is how a background
    // thread would actually propagate it.
    let boxed: Box<dyn Error> = Box::new(err);
    assert!(boxed.to_string().contains("not delivered"));
}
