//! [`PowerPreference`]: which GPU a host asks for.

/// Which GPU a host asks for where a machine has two, as a hybrid laptop's integrated and discrete
/// ones. A palantir type, as [`Vsync`](crate::Vsync) and [`TargetFormat`](crate::TargetFormat)
/// are, so a host builder names no wgpu type; `crate::gpu` converts it to wgpu's. Refused: wgpu's
/// `None`, which leaves the pick to wgpu, where a host says what it wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PowerPreference {
    /// The GPU that draws least power, an integrated one; a window's default, as a UI idles most
    /// of its life.
    #[default]
    LowPower,
    /// The fastest GPU, a discrete one; the headless and bench paths' pick, as each times or
    /// renders as fast as it can.
    HighPerformance,
}

impl From<PowerPreference> for wgpu::PowerPreference {
    fn from(preference: PowerPreference) -> Self {
        match preference {
            PowerPreference::LowPower => Self::LowPower,
            PowerPreference::HighPerformance => Self::HighPerformance,
        }
    }
}
