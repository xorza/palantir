//! Observability for the shaped-buffer cache. Built on
//! [`TestOnly`](crate::common::counters::TestOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! ## What this exists to separate
//!
//! Every entry point into [`CosmicMeasure`] returns a measurement
//! whether it reshaped or answered from cache, so from outside the two
//! are indistinguishable — and reshaping is the entire cost the cache
//! exists to avoid. [`TextShaper::measure_calls`] counts *dispatches* a
//! layer up and explicitly does not answer this; its own doc says
//! "cosmic may still hit its shaped-buffer cache, so the counter tracks
//! dispatches, not reshapes".
//!
//! Retention needs the same treatment. A buffer's lifetime turns on
//! three events — inserted, looked up, superseded — and which of them
//! fired is what separates a resize drag from a scroll, not how many
//! buffers happen to be resident afterwards.
//!
//! Counters accumulate for the life of the shaper, which outlives any
//! one frame and is shared across windows, so readers take a delta.
//!
//! [`CosmicMeasure`]: crate::text::cosmic::CosmicMeasure
//! [`TextShaper::measure_calls`]: crate::text::shaper::TextShaper

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    /// What the shaped-buffer cache did.
    ///
    /// The counters are reached through directly rather than through
    /// per-field forwarders: [`TestOnly`] already owns the gate, so
    /// `probe.hits.bump()` is the whole call site and a wrapper would only
    /// restate the field name.
    ///
    /// [`TestOnly`]: crate::common::counters::TestOnly
    pub(crate) struct CacheCounters;

    /// One reading of a [`CacheCounters`]'s tallies. Subtract two to get
    /// what a span of frames did — the counters accumulate for the life of
    /// the shaper, which outlives any one frame and is shared across
    /// windows. Copied out rather than borrowed so a test can hold a
    /// "before" reading across calls that need the shaper again.
    pub(crate) struct CacheCounts;

    /// Runs actually pushed through cosmic — `set_text` plus
    /// `shape_until_scroll`. The cost every other counter here exists to
    /// explain.
    shapes,
    /// Lookups answered from the cache, layout-side and render-side
    /// alike.
    hits,
    /// Entries demoted to the probation window because the reuse slot
    /// that owned them moved on to a different key.
    supersedes,
    /// Buffers dropped by the end-of-frame sweep.
    expiries,
    /// Times the "…" advance had to be reshaped because no slot held
    /// that face. Separate from `shapes`, which counts runs that landed
    /// in the cache — the ellipsis probe shapes without inserting.
    ellipsis_misses,
}
