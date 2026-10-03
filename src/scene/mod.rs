//! Recorded scene state. [`forest::Forest`] owns one arena per layer;
//! [`crate::cascade`] and [`crate::damage`] turn that recording into the
//! immutable per-frame data consumed by input and rendering.
//! [`record_store`] retains the variable-sized payloads referenced by
//! recorded shapes.

pub(crate) mod endpoint;
pub(crate) mod forest;
pub(crate) mod layer;
pub(crate) mod node;
pub(crate) mod per_layer;
pub(crate) mod record_store;
pub(crate) mod seen_ids;
pub(crate) mod tree;
