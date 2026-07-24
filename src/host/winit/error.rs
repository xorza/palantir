use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::window::WindowToken;

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
    /// No graphics adapter matched the requested surface and power policy.
    RequestAdapter { source: wgpu::RequestAdapterError },
    /// The selected adapter could not create the logical device.
    RequestDevice { source: wgpu::RequestDeviceError },
    /// Aperture was compiled without a wgpu backend for the current target.
    NoGpuBackend,
    /// The selected adapter lacks a feature required by Aperture.
    MissingAdapterFeatures {
        required: wgpu::Features,
        supported: wgpu::Features,
    },
    /// One requested device limit exceeds what the selected adapter supports.
    UnsupportedAdapterLimit {
        name: &'static str,
        required: u64,
        supported: u64,
    },
    /// The selected adapter cannot present to this window's surface.
    IncompatibleSurface,
    /// The surface cannot satisfy Aperture's linear-to-sRGB output contract.
    MissingSrgbSurface,
    /// The surface lacks texture usages required by Aperture's compositor.
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
            Self::RequestAdapter { source } => {
                write!(f, "failed to find a compatible graphics adapter: {source}")
            }
            Self::RequestDevice { source } => {
                write!(f, "failed to create the graphics device: {source}")
            }
            Self::NoGpuBackend => {
                f.write_str("Aperture was compiled without a GPU backend for this target")
            }
            Self::MissingAdapterFeatures {
                required,
                supported,
            } => write!(
                f,
                "graphics adapter is missing required features \
                 (required: {required:?}, supported: {supported:?})"
            ),
            Self::UnsupportedAdapterLimit {
                name,
                required,
                supported,
            } => write!(
                f,
                "graphics adapter limit {name} is {supported}, but Aperture requires {required}"
            ),
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

impl Error for WinitHostError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateEventLoop { source } | Self::RunEventLoop { source } => Some(source),
            Self::CreateWindow { source, .. } => Some(source),
            Self::CreateSurface { source } => Some(source),
            Self::RequestAdapter { source } => Some(source),
            Self::RequestDevice { source } => Some(source),
            Self::NoGpuBackend
            | Self::MissingAdapterFeatures { .. }
            | Self::UnsupportedAdapterLimit { .. }
            | Self::IncompatibleSurface
            | Self::MissingSrgbSurface
            | Self::MissingSurfaceUsages { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use crate::host::winit::error::WinitHostError;

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

        let capability = WinitHostError::UnsupportedAdapterLimit {
            name: "max_immediate_size",
            required: 16,
            supported: 8,
        };
        assert_eq!(
            capability.to_string(),
            "graphics adapter limit max_immediate_size is 8, but Aperture requires 16"
        );
        assert!(capability.source().is_none());
    }
}
