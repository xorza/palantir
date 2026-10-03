//! What a handle reports once the event loop it addressed has exited.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::gpu::error::SurfaceError;
use crate::window::window_token::WindowToken;
use std::fmt;
use winit::error;

/// The event loop has exited, so a [`HostHandle`](crate::HostHandle) can no
/// longer deliver to it.
///
/// Reported by [`HostHandle::run_on_main`](crate::HostHandle::run_on_main)
/// alone, because it is the only poke carrying **owned work**: a lost send
/// destroys the closure and the application-state mutation it would have
/// performed, which the caller has no other way to observe. `request_repaint`
/// and `quit` carry no payload — losing either against a loop that is already
/// leaving costs nothing — so they stay fire-and-forget.
///
/// Zero-sized: there is exactly one way to fail, and the closure is not handed
/// back because there would be no `&mut T` left to run it against. Its
/// captures drop with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostDisconnected;

impl Display for HostDisconnected {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("host event loop has exited; the scheduled work was not delivered")
    }
}

impl Error for HostDisconnected {}

/// Failure while constructing or running a [`WinitHost`](crate::WinitHost).
#[derive(Debug)]
#[non_exhaustive]
pub enum WinitHostError {
    /// Winit could not create the application event loop.
    CreateEventLoop {
        /// What winit reported.
        source: error::EventLoopError,
    },
    /// Winit's event loop terminated with an error.
    RunEventLoop {
        /// What winit reported.
        source: error::EventLoopError,
    },
    /// The operating system could not create a native window.
    CreateWindow {
        /// The window that failed to open.
        token: WindowToken,
        /// What the platform reported.
        source: error::OsError,
    },
    /// A native window has no surface Palantir can present through.
    Surface {
        /// The window whose surface failed.
        token: WindowToken,
        /// What went wrong with it.
        source: SurfaceError,
    },
}

impl Display for WinitHostError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateEventLoop { source } => write!(f, "failed to create event loop: {source}"),
            Self::RunEventLoop { source } => write!(f, "event loop failed: {source}"),
            Self::CreateWindow { token, source } => {
                write!(f, "failed to create window {token:?}: {source}")
            }
            Self::Surface { token, source } => {
                write!(f, "window {token:?} surface failed: {source}")
            }
        }
    }
}

impl Error for WinitHostError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateEventLoop { source } | Self::RunEventLoop { source } => Some(source),
            Self::CreateWindow { source, .. } => Some(source),
            Self::Surface { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
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
}
