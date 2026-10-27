//! Recorded scene state. [`forest::Forest`] owns one arena per plane; [`crate::cascade`] and [`crate::damage`] turn it into the per-frame data input and rendering consume; the variable-sized payloads recorded shapes reference sit in [`crate::shape::record_store`].

pub(crate) mod bar_geometry;
pub(crate) mod endpoint;
pub(crate) mod forest;
pub(crate) mod layer;
pub(crate) mod node;
pub(crate) mod per_plane;
pub(crate) mod plane;
pub(crate) mod scrollbars_def;
pub(crate) mod seen_ids;
pub(crate) mod tree;
