//! [`GpuWait`]: how a wait on the device ended.

/// How a wait on the device ended, as the hosts' `wait_for_gpu` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuWait {
    /// The device finished every submission, and the copies the wait was for came back: mapped,
    /// or failed to map.
    Done,
    /// The device was lost, before the wait or during it.
    DeviceLost,
    /// The device did not finish within the wait's bound, and is not lost: a hang no system has
    /// reset yet.
    TimedOut,
}
