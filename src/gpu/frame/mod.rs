//! One frame on the device: the pass plan, the step schedule, the submission, the
//! timestamp queries, the debug overlay, and the capture labels.

pub(crate) mod binder;
pub(crate) mod debug_marker;
pub(crate) mod debug_overlay;
pub(crate) mod frame_passes;
pub(crate) mod gpu_timings;
pub(crate) mod main_segment;
pub(crate) mod schedule;
pub(crate) mod submission;
