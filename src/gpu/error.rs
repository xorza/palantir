//! What the device request and the window surface report when they fail.

use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};

/// A driver failure kept whole but unnamed, so no graphics-API type reaches the published surface.
#[derive(Debug)]
pub struct DriverError(Box<dyn Error + Send + Sync + 'static>);

impl DriverError {
    pub(crate) fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

impl Display for DriverError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Error for DriverError {
    /// Skips the driver error itself; the holding variant prints its words.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.0.source()
    }
}

/// A device or adapter cannot run Palantir's pipelines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmetRequirements {
    /// A required feature is absent.
    Features {
        /// The features needed and missing.
        missing: String,
    },
    /// A limit sits below the floor Palantir needs.
    Limit {
        /// The device-limit field name.
        name: &'static str,
        /// The floor Palantir needs.
        required: u64,
        /// What the device offers.
        available: u64,
    },
}

impl Display for UnmetRequirements {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Features { missing } => write!(
                f,
                "graphics device is missing {missing}, which Palantir requires"
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
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuRequestError {
    /// Palantir was compiled without a wgpu backend for the current target.
    NoBackend,
    /// No graphics adapter matched the requested power policy.
    RequestAdapter {
        /// What the driver reported.
        source: DriverError,
    },
    /// The adapter that answered cannot run Palantir's pipelines.
    Requirements {
        /// Which requirement went unmet.
        source: UnmetRequirements,
    },
    /// The adapter could not create the logical device.
    RequestDevice {
        /// What the driver reported.
        source: DriverError,
    },
}

impl Display for GpuRequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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

/// A native window surface Palantir cannot present through. Separate from
/// [`GpuRequestError`] because a surface is per window.
#[cfg(feature = "winit")]
#[derive(Debug)]
#[non_exhaustive]
pub enum SurfaceError {
    /// The platform could not create a presentation surface for the window.
    Create {
        /// What the driver reported.
        source: DriverError,
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
        /// The usages the compositor needs and the surface lacks.
        missing: String,
    },
}

#[cfg(feature = "winit")]
impl From<GpuRequestError> for SurfaceError {
    fn from(source: GpuRequestError) -> Self {
        Self::Device { source }
    }
}

#[cfg(feature = "winit")]
#[cfg(feature = "winit")]
impl Display for SurfaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create { source } => write!(f, "failed to create a window surface: {source}"),
            Self::Device { source } => Display::fmt(source, f),
            Self::Incompatible => {
                f.write_str("graphics adapter cannot present to the window surface")
            }
            Self::MissingSrgb => f.write_str("window surface has no sRGB format and color space"),
            Self::MissingUsages { missing } => write!(
                f,
                "window surface cannot do {missing}, which Palantir's compositor needs"
            ),
        }
    }
}

#[cfg(feature = "winit")]
impl Error for SurfaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Create { source } => Some(source),
            // Skips the inner itself; its `Display` forwards.
            Self::Device { source } => source.source(),
            Self::Incompatible | Self::MissingSrgb | Self::MissingUsages { .. } => None,
        }
    }
}

/// The set flags of a graphics-API bitfield, by name; takes an iterator as the callers' flag types are unrelated.
pub(crate) fn flag_names<T>(names: impl Iterator<Item = (&'static str, T)>) -> String {
    names.map(|(name, _)| name).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_names_reports_the_names_alone() {
        let missing = flag_names(wgpu::Features::IMMEDIATES.iter_names());
        assert_eq!(missing, "IMMEDIATES");

        let usages = wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC;
        assert_eq!(flag_names(usages.iter_names()), "COPY_SRC, COPY_DST");

        assert_eq!(
            flag_names(wgpu::TextureUsages::empty().iter_names()),
            "",
            "nothing missing reads as nothing, not as a bare wrapper"
        );
    }
}
