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
    assert_eq!(event_loop.to_string(), "failed to create event loop");
    assert_eq!(
        event_loop.source().map(ToString::to_string),
        Some("EventLoop can't be recreated".to_owned()),
        "the cause rides in the chain, not in the sentence"
    );

    // A device request failing during the first window's open reads as a device failure; it hands on its cause, not itself, so chain printers never repeat a sentence.
    let unmet = UnmetRequirements::Limit {
        name: "max_immediate_size",
        required: 16,
        available: 8,
    };
    let capability = WinitHostError::Gpu {
        source: GpuRequestError::Requirements {
            source: unmet.clone(),
        },
    };
    assert_eq!(
        capability.to_string(),
        "the graphics adapter cannot run Palantir"
    );
    assert_eq!(
        capability.source().map(ToString::to_string),
        Some(unmet.to_string()),
        "the chain skips the request error, which added no words of its own",
    );

    // A loss with no new device says so, and keeps the request error, whose words it adds to.
    let lost = WinitHostError::DeviceLost {
        source: GpuRequestError::NoBackend,
    };
    assert_eq!(
        lost.to_string(),
        "graphics device lost and none opened in its place"
    );
    assert_eq!(
        lost.source().map(ToString::to_string),
        Some(GpuRequestError::NoBackend.to_string()),
    );

    let kept_lost = WinitHostError::DeviceKeptLost;
    assert_eq!(
        kept_lost.to_string(),
        "graphics device lost, and every device attached in its place was lost again",
    );
    assert!(kept_lost.source().is_none());

    let surface = WinitHostError::Surface {
        token: WindowToken(7),
        source: SurfaceError::MissingSrgb,
    };
    assert_eq!(surface.to_string(), "window WindowToken(7) surface failed");
    assert_eq!(
        surface.source().map(ToString::to_string),
        Some(SurfaceError::MissingSrgb.to_string()),
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
