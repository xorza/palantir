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

    // A device request failing during a window's surface open arrives as that window's surface error; it hands on its cause, not itself, so chain printers never repeat a sentence.
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
    // Returned by value during shutdown, so no payload.
    assert_eq!(size_of::<HostDisconnected>(), 0);

    // The message names the consequence (work was thrown away), not just "event loop exited".
    let err = HostDisconnected;
    assert_eq!(
        err.to_string(),
        "host event loop has exited; the scheduled work was not delivered"
    );
    assert!(err.source().is_none());

    let boxed: Box<dyn Error> = Box::new(err);
    assert!(boxed.to_string().contains("not delivered"));
}
