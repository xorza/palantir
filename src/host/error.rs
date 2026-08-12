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

/// Failure while standing up a [`HeadlessGpu`](crate::HeadlessGpu).
#[derive(Debug)]
#[non_exhaustive]
pub enum HeadlessGpuError {
    /// Palantir was compiled without a wgpu backend for the current target.
    NoBackend,
    /// No graphics adapter matched the requested power policy.
    RequestAdapter { source: wgpu::RequestAdapterError },
    /// The adapter that answered cannot run Palantir's pipelines.
    Requirements { source: UnmetRequirements },
    /// The adapter could not create the logical device.
    RequestDevice { source: wgpu::RequestDeviceError },
}

impl Display for HeadlessGpuError {
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

impl Error for HeadlessGpuError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NoBackend => None,
            Self::RequestAdapter { source } => Some(source),
            Self::Requirements { source } => Some(source),
            Self::RequestDevice { source } => Some(source),
        }
    }
}
