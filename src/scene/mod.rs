//! Recorded and derived scene state. [`forest::Forest`] owns one arena
//! per layer; [`cascade`] and [`damage`] turn that recording into the
//! immutable per-frame data consumed by input and rendering.
//! [`record_store`] retains the variable-sized payloads referenced by
//! recorded shapes.

pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod forest;
pub(crate) mod layer;
pub(crate) mod node;
pub(crate) mod record_store;
pub(crate) mod seen_ids;
pub(crate) mod shapes;
pub(crate) mod tree;
pub(crate) mod visibility;
