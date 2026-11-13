//! One frame on the device: the pass plan, the step schedule, the submission, the timestamp
//! queries and whether a host times them, the debug overlay, and the capture labels.

pub(crate) mod debug_overlay;
pub(crate) mod frame_passes;
pub(crate) mod gpu_timings;
pub(crate) mod main_segment;
pub(crate) mod schedule;
pub(crate) mod submission;
