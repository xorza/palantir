//! What a handle reports once the event loop it addressed has exited.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::host::error::GpuRequestError;
use crate::window::window_token::WindowToken;

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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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
        source: winit::error::EventLoopError,
    },
    /// Winit's event loop terminated with an error.
    RunEventLoop {
        source: winit::error::EventLoopError,
    },
    /// The operating system could not create a native window.
    CreateWindow {
        token: WindowToken,
        source: winit::error::OsError,
    },
    /// Wgpu could not create a presentation surface for a native window.
    CreateSurface { source: wgpu::CreateSurfaceError },
    /// Asking the driver for a device failed — the same four ways it fails
    /// for every host, so the same enum describes them.
    Gpu { source: GpuRequestError },
    /// The selected adapter cannot present to this window's surface.
    IncompatibleSurface,
    /// The surface cannot satisfy Palantir's linear-to-sRGB output contract.
    MissingSrgbSurface,
    /// The surface lacks texture usages required by Palantir's compositor.
    MissingSurfaceUsages {
        required: wgpu::TextureUsages,
        supported: wgpu::TextureUsages,
    },
}

impl Display for WinitHostError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateEventLoop { source } => write!(f, "failed to create event loop: {source}"),
            Self::RunEventLoop { source } => write!(f, "event loop failed: {source}"),
            Self::CreateWindow { token, source } => {
                write!(f, "failed to create window {token:?}: {source}")
            }
            Self::CreateSurface { source } => {
                write!(f, "failed to create window surface: {source}")
            }
            Self::Gpu { source } => Display::fmt(source, f),
            Self::IncompatibleSurface => {
                f.write_str("graphics adapter cannot present to the window surface")
            }
            Self::MissingSrgbSurface => {
                f.write_str("window surface has no sRGB format and color space")
            }
            Self::MissingSurfaceUsages {
                required,
                supported,
            } => write!(
                f,
                "window surface lacks required texture usages \
                 (required: {required:?}, supported: {supported:?})"
            ),
        }
    }
}

impl From<GpuRequestError> for WinitHostError {
    fn from(source: GpuRequestError) -> Self {
        Self::Gpu { source }
    }
}

impl Error for WinitHostError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateEventLoop { source } | Self::RunEventLoop { source } => Some(source),
            Self::CreateWindow { source, .. } => Some(source),
            Self::CreateSurface { source } => Some(source),
            // The inner's own source, skipping the inner itself. This
            // variant adds no words of its own — its `Display` forwards —
            // so chaining it would print the identical sentence twice.
            Self::Gpu { source } => source.source(),
            Self::IncompatibleSurface
            | Self::MissingSrgbSurface
            | Self::MissingSurfaceUsages { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use crate::host::error::{GpuRequestError, UnmetRequirements};
    use crate::host::winit::error::{HostDisconnected, WinitHostError};

    #[test]
    fn host_errors_preserve_sources_and_explain_capability_failures() {
        let event_loop = WinitHostError::CreateEventLoop {
            source: winit::error::EventLoopError::RecreationAttempt,
        };
        assert_eq!(
            event_loop.to_string(),
            "failed to create event loop: EventLoop can't be recreated"
        );
        assert!(event_loop.source().is_some());

        // A device request is the shared type's to describe. The host
        // forwards its words and hands on *its* cause rather than itself,
        // so a chain printer never meets the same sentence twice.
        let unmet = UnmetRequirements::Limit {
            name: "max_immediate_size",
            required: 16,
            available: 8,
        };
        let capability = WinitHostError::from(GpuRequestError::Requirements {
            source: unmet.clone(),
        });
        assert_eq!(
            capability.to_string(),
            "the graphics adapter cannot run Palantir: \
             graphics device limit max_immediate_size is 8, but Palantir requires 16",
        );
        assert_eq!(
            capability.source().map(ToString::to_string),
            Some(unmet.to_string()),
            "the chain skips the wrapper, which added no words of its own",
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
