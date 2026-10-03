//! One frame on the device: the step schedule, the submission, the
//! timestamp queries, the debug overlay, and the capture labels.

pub(crate) mod debug_marker;
pub(crate) mod gpu_timings;
pub(crate) mod overlay_pass;
pub(crate) mod schedule;
pub(crate) mod submission;
