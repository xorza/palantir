//! Which adapter to open, when the machine offers more than one.

/// Which GPU a device request prefers on a machine that has a choice.
///
/// Palantir's own word for the policy, so a caller states it without naming
/// a graphics-API type. A hybrid laptop is the case it exists for: the
/// integrated GPU draws a user interface without waking the discrete one,
/// while a benchmark is worth little unless it runs on the adapter a person
/// is looking at.
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
