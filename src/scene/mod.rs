//! Recorded scene state. [`forest::Forest`] owns one arena per layer; [`crate::cascade`] and [`crate::damage`] turn it into the per-frame data input and rendering consume; [`record_store`] retains the variable-sized payloads recorded shapes reference.

pub(crate) mod endpoint;
pub(crate) mod forest;
pub(crate) mod layer;
pub(crate) mod node;
pub(crate) mod per_layer;
pub(crate) mod record_store;
pub(crate) mod seen_ids;
pub(crate) mod tree;
