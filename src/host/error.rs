//! Failures common to every host, rather than to one of them.

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
        required: wgpu::Features,
        available: wgpu::Features,
    },
    /// A limit Palantir needs raised sits below the floor.
    Limit {
        name: &'static str,
        required: u64,
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
    RequestAdapter { source: wgpu::RequestAdapterError },
    /// The adapter that answered cannot run Palantir's pipelines.
    Requirements { source: UnmetRequirements },
    /// The adapter could not create the logical device.
    RequestDevice { source: wgpu::RequestDeviceError },
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
