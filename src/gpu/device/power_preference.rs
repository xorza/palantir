//! Which adapter to open, when the machine offers more than one.

/// Which GPU a device request prefers when the machine has a choice, so callers need no graphics-API type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PowerPreference {
    /// Take whichever adapter the driver ranks first.
    #[default]
    Any,
    /// Prefer the integrated GPU: less power, less throughput.
    LowPower,
    /// Prefer the discrete GPU.
    HighPerformance,
}
