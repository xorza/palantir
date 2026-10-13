//! Intrinsic-dimensions queries: the on-demand `LenReq` API. Owns the query types (`LenReq`,
//! `IntrinsicQuery`, the ranges a walk answers with) and [`IntrinsicPass`](intrinsic_pass::IntrinsicPass),
//! which memoizes each answer and, on a miss, walks the subtree: `Sizing` overrides, padding/margin
//! and `min_size`/`max_size` clamps around each driver's `intrinsic()` for content-driven sizes,
//! and leaf intrinsics.
//!
//! Per-driver intrinsic logic lives beside that driver's `measure`/`arrange`, in its
//! [`LayoutDriver`](crate::layout::drivers::LayoutDriver) impl.

pub(crate) mod intrinsic_pass;
pub(crate) mod intrinsic_query;
pub(crate) mod intrinsic_range;
pub(crate) mod intrinsic_walk;
pub(crate) mod len_req;

#[cfg(test)]
mod tests;
