//! What a handle reports once the event loop it addressed has exited.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::gpu::error::SurfaceError;
use crate::window::window_token::WindowToken;
use std::fmt;
use winit::error;

/// The event loop has exited, so a [`HostHandle`](crate::HostHandle) can no longer deliver to it.
///
/// Reported only by [`HostHandle::run_on_main`](crate::HostHandle::run_on_main), the one poke carrying **owned work**: a lost send destroys the closure and its state mutation unobservably. `request_repaint` and `quit` carry no payload and stay fire-and-forget.
///
/// Zero-sized; the closure isn't handed back since no `&mut T` remains to run it against.
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
mod tests;
