//! What the device request and the window surface report when they fail.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// A device or adapter cannot run Palantir's pipelines.
///
/// Raised against an adapter while
/// [negotiating](crate::DeviceRequirements::negotiate) what to ask for, and
/// against a finished device when a host is handed one it cannot use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmetRequirements {
    /// A feature Palantir cannot run without is absent.
    Features {
        /// What Palantir needs.
        required: wgpu::Features,
        /// What the device offers.
        available: wgpu::Features,
    },
    /// A limit Palantir needs raised sits below the floor.
    Limit {
        /// The `wgpu::Limits` field, by its own name.
        name: &'static str,
        /// The floor Palantir needs.
        required: u64,
        /// What the device offers.
        available: u64,
    },
}

impl Display for UnmetRequirements {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Features {
                required,
                available,
            } => write!(
                f,
                "graphics device is missing {:?}, which Palantir requires (it has {available:?})",
                *required - *available
            ),
            Self::Limit {
                name,
                required,
                available,
            } => write!(
                f,
                "graphics device limit {name} is {available}, but Palantir requires {required}"
            ),
        }
    }
}

impl Error for UnmetRequirements {}

/// Failure while asking a driver for a device.
///
/// One enum for every host, because every host takes the same four steps
/// and fails them in the same four ways. Two enums is what let the same
/// missing backend be reported as "no wgpu backend is compiled in for this
/// target" through one door and "Palantir was compiled without a GPU backend
/// for this target" through the other.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuRequestError {
    /// Palantir was compiled without a wgpu backend for the current target.
    NoBackend,
    /// No graphics adapter matched the requested power policy.
    RequestAdapter {
        /// What wgpu reported.
        source: wgpu::RequestAdapterError,
    },
    /// The adapter that answered cannot run Palantir's pipelines.
    Requirements {
        /// Which requirement went unmet.
        source: UnmetRequirements,
    },
    /// The adapter could not create the logical device.
    RequestDevice {
        /// What wgpu reported.
        source: wgpu::RequestDeviceError,
    },
}

impl Display for GpuRequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBackend => f.write_str("no wgpu backend is compiled in for this target"),
            Self::RequestAdapter { source } => {
                write!(f, "failed to find a graphics adapter: {source}")
            }
            Self::Requirements { source } => {
                write!(f, "the graphics adapter cannot run Palantir: {source}")
            }
            Self::RequestDevice { source } => {
                write!(f, "failed to create the graphics device: {source}")
            }
        }
    }
}

impl Error for GpuRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NoBackend => None,
            Self::RequestAdapter { source } => Some(source),
            Self::Requirements { source } => Some(source),
            Self::RequestDevice { source } => Some(source),
        }
    }
}

/// A native window surface Palantir cannot present through.
///
/// Separate from [`GpuRequestError`] because a surface is per window, while
/// the device request happens once: a second window can fail this on a device
/// that already opened successfully.
#[cfg(feature = "winit")]
#[derive(Debug)]
#[non_exhaustive]
pub enum SurfaceError {
    /// The platform could not create a presentation surface for the window.
    Create {
        /// What the graphics driver reported.
        source: wgpu::CreateSurfaceError,
    },
    /// Opening the device for this surface failed.
    Device {
        /// Which of the four ways the request failed.
        source: GpuRequestError,
    },
    /// The selected adapter cannot present to this window's surface.
    Incompatible,
    /// The surface cannot satisfy Palantir's linear-to-sRGB output contract.
    MissingSrgb,
    /// The surface lacks texture usages Palantir's compositor needs.
    MissingUsages {
        /// What the compositor needs.
        required: wgpu::TextureUsages,
        /// What the surface offers.
        supported: wgpu::TextureUsages,
    },
}

#[cfg(feature = "winit")]
impl From<GpuRequestError> for SurfaceError {
    fn from(source: GpuRequestError) -> Self {
        Self::Device { source }
    }
}

#[cfg(feature = "winit")]
impl From<wgpu::CreateSurfaceError> for SurfaceError {
    fn from(source: wgpu::CreateSurfaceError) -> Self {
        Self::Create { source }
    }
}

#[cfg(feature = "winit")]
impl Display for SurfaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create { source } => write!(f, "failed to create a window surface: {source}"),
            Self::Device { source } => Display::fmt(source, f),
            Self::Incompatible => {
                f.write_str("graphics adapter cannot present to the window surface")
            }
            Self::MissingSrgb => f.write_str("window surface has no sRGB format and color space"),
            Self::MissingUsages {
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

#[cfg(feature = "winit")]
impl Error for SurfaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Create { source } => Some(source),
            // The inner's own source, skipping the inner itself. This
            // variant adds no words of its own — its `Display` forwards —
            // so chaining it would print the identical sentence twice.
            Self::Device { source } => source.source(),
            Self::Incompatible | Self::MissingSrgb | Self::MissingUsages { .. } => None,
        }
    }
}
