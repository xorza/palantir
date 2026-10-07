//! Observability for the shaped-buffer cache, built on [`TestOnly`](crate::common::counters::TestOnly). [`CosmicMeasure`] entry points return a measurement whether they reshaped or hit the cache, and [`TextShaper::measure_calls`] counts dispatches, not reshapes; these counters separate them, and the inserted / looked-up / superseded events that tell a resize drag from a scroll. They accumulate, since the shaper outlives any frame and is shared across windows.
//!
//! [`CosmicMeasure`]: crate::text::cosmic::CosmicMeasure
//! [`TextShaper::measure_calls`]: crate::text::shaper::TextShaper

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells TestOnly, reads cfg(test);

    /// What the shaped-buffer cache did; counters are reached directly, as [`TestOnly`] owns the gate.
    ///
    /// [`TestOnly`]: crate::common::counters::TestOnly
    pub(crate) struct CacheCounters;

    /// One reading of a [`CacheCounters`]'s tallies; subtract two to get what a span of frames did. Copied out so a test can hold a "before" across calls.
    pub(crate) struct CacheCounts;

    /// Runs actually pushed through cosmic (`set_text` plus `shape_until_scroll`), the cost the other counters explain.
    shapes: u32,
    /// Lookups answered from the cache, layout-side and render-side.
    hits: u32,
    /// Entries demoted to the probation window because their reuse slot moved to a different key.
    supersedes: u32,
    /// Buffers dropped by the end-of-frame sweep.
    expiries: u32,
    /// Times the "…" advance was reshaped because no slot held that face; separate from `shapes`, since the probe shapes without inserting.
    ellipsis_misses: u32,
}
