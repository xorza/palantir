//! What a handle reports once the event loop it addressed has exited.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::gpu::error::{GpuRequestError, SurfaceError};
use crate::window::window_token::WindowToken;
use std::fmt;
use winit::error;

/// The event loop has exited, so a [`HostHandle`](crate::HostHandle) can no longer deliver to it.
///
/// Reported only by [`HostHandle::run_on_main`](crate::HostHandle::run_on_main), the one poke carrying **owned work**: a lost send destroys the closure and its state mutation unobservably. `quit` carries no payload and stays fire-and-forget.
///
/// Zero-sized; the closure isn't handed back since no `&mut T` remains to run it against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostHandleError;

impl Display for HostHandleError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("host event loop has exited; the scheduled work was not delivered")
    }
}

impl Error for HostHandleError {}

/// Failure while constructing or running a [`WinitHost`](crate::WinitHost). A variant that holds
/// a cause names it in [`Error::source`], never in its `Display`, so a reporter that walks the
/// chain prints each sentence once.
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
    /// No graphics device Palantir can run on was found for the first window.
    Gpu {
        /// Which of the four ways the request failed.
        source: GpuRequestError,
    },
    /// The graphics device was lost, as a GPU reset, a driver update or a removed GPU loses it,
    /// and no new one opened in its place.
    DeviceLost {
        /// Why the new device did not open.
        source: GpuRequestError,
    },
    /// The graphics device was lost, and every device attached in its place during the
    /// recovery's 5 s was lost again.
    DeviceKeptLost,
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
            Self::CreateEventLoop { .. } => f.write_str("failed to create event loop"),
            Self::RunEventLoop { .. } => f.write_str("event loop failed"),
            Self::CreateWindow { token, .. } => write!(f, "failed to create window {token:?}"),
            Self::Gpu { source } => Display::fmt(source, f),
            Self::DeviceLost { .. } => {
                f.write_str("graphics device lost and none opened in its place")
            }
            Self::DeviceKeptLost => f.write_str(
                "graphics device lost, and every device attached in its place was lost again",
            ),
            Self::Surface { token, .. } => write!(f, "window {token:?} surface failed"),
        }
    }
}

impl Error for WinitHostError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateEventLoop { source } | Self::RunEventLoop { source } => Some(source),
            Self::CreateWindow { source, .. } => Some(source),
            // Skips the request error itself, which adds no words to the `Display` it forwards.
            Self::Gpu { source } => source.source(),
            Self::DeviceLost { source } => Some(source),
            Self::DeviceKeptLost => None,
            Self::Surface { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests;
