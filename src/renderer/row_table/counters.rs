//! Observability for the gradient row table, built on [`BenchOnly`]. A row id
//! does not say how [`RowTable::register`] reached it (index hit,
//! free-row bake, eviction bake, doubling, magenta fallback), which cost very
//! differently. Counters accumulate rather than reset, as the table is shared
//! across windows with no single pass to scope a reset to.
//!
//! [`BenchOnly`]: crate::common::counters::BenchOnly
//! [`RowTable::register`]:
//!     crate::renderer::row_table::RowTable::register

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells BenchOnly, reads cfg(feature = "internals");

    pub(super) struct RowTableCounters;

    /// One reading; subtract two for a span's activity.
    pub(super) struct RowTableCounts;

    /// `register` calls, however they resolved.
    registrations: u32,
    /// Calls answered from the index without a bake.
    hits: u32,
    /// Rows claimed for a ramp (free rows and evictions), each baked at the next flush: one per
    /// miss.
    bakes: u32,
    /// Bakes that displaced a resident gradient, which re-bakes if it returns.
    evictions: u32,
    /// Capacity doublings.
    growths: u32,
    /// Capacity halvings, each at the end of an idle expiry window.
    halvings: u32,
    /// LUT rows handed to the GPU, summed over flushes: each flush's dirty runs, with clean gaps
    /// of up to `MERGE_GAP_ROWS` folded in, so compare with [`Self::bakes`].
    rows_uploaded: u32,
    /// Registrations that resolved to
    /// [`LutRow::FALLBACK`](crate::primitives::paint::lut_row::LutRow): table full and capped.
    fallbacks: u32,
}
