//! Fixtures whose scene changes every frame.
//!
//! Still-frame fixtures never reach the insert, supersede or expiry paths of
//! the layout measure cache, reuse rows, shaped-buffer cache, encoded-run
//! cache and glyph atlas. These drive the record-side shapes that do: a width
//! drag, changing text, and rows entering and leaving. A scale ramp lives in
//! `gates::on_gpu::scale_ramp_rasterizes_at_a_flat_cost_per_frame`, since it
//! needs a device.
//!
//! Budgets are not all zero: reshaping new content costs roughly ten
//! allocations per run inside `cosmic_text`, while Palantir's own scratch
//! stays quiet. A budget pins flatness: all 64 frames are checked, so a cost
//! growing with gesture length blows the ceiling. Tighten a number when a
//! change lowers it; one that must rise is the regression to catch.

use crate::harness::Audit;
use palantir::{Configure, Panel, Sizing, Text, TextWrap, Ui};
use std::fmt::Write as _;
use std::hint;
use std::ops::Range;

/// Labels per churning fixture.
const ROWS: u32 = 8;

/// What one frame of either width drag may allocate: a band over the measured
/// worst frame, so a one-block shift is not a failure. A leak of one
/// allocation per run costs [`ROWS`] blocks, well above it. Shared because the
/// two drags are one workload under two wrap policies.
const DRAG_BLOCKS_PER_FRAME_MAX: u64 = 20;

/// A resize drag: the committed width moves every frame, so each run resolves
/// to a fresh bounded key, supersedes the old one, and mints a shaped buffer
/// nothing asks for again (`shaped_buffer_cache::PROBATION_KEEP_FRAMES`).
#[test]
fn width_drag_stays_flat() {
    // One shaped buffer per run, all inside cosmic.
    width_drag(
        TextWrap::Wrap,
        "a label long enough to need wrapping at this width",
        240.0,
    );
}

/// The same drag against a truncating policy: `measure_truncated` re-cuts
/// against the cached unbounded probe and reshapes only the prefix.
#[test]
fn ellipsis_width_drag_stays_flat() {
    // Cheaper than the wrapping drag typically, dearer at its worst:
    // `shape_truncated` retires clusters while the prefix overruns, so a
    // back-off frame reshapes several times.
    width_drag(
        TextWrap::Ellipsis,
        "a label far too long for the column it sits in",
        180.0,
    );
}

/// [`ROWS`] copies of `label` in a column whose width steps a whole pixel per
/// frame from `from`, as a drag commits after quantizing.
#[track_caller]
fn width_drag(wrap: TextWrap, label: &'static str, from: f32) {
    let mut step = 0u32;
    Audit::new()
        .text()
        .budget(DRAG_BLOCKS_PER_FRAME_MAX)
        .run(move |ui| {
            let width = from + (step % 64) as f32;
            step += 1;
            Panel::vstack()
                .id_salt("drag-root")
                .size((Sizing::fixed(width), Sizing::FILL))
                .show(ui, |ui| {
                    for row in 0..ROWS {
                        Text::new(label).id_salt(row).text_wrap(wrap).show(ui);
                    }
                });
        });
}

/// Rows entering and leaving the tree (virtualized list): the measure cache's
/// descriptor sequence changes, reuse rows are swept against `removed`, and
/// widget-keyed maps churn.
#[test]
fn scrolling_row_window_alloc_free() {
    let mut first = 0u32;
    Audit::new().run(move |ui| {
        first += 1;
        rows(ui, Sizing::FILL, first..first + ROWS, 20.0);
    });
}

/// Widget count oscillating: ids are added and removed, so `removed` is
/// non-empty on shrink frames and per-widget maps take their eviction path.
#[test]
fn widget_add_remove_stays_flat() {
    let mut step = 0u32;
    // Explicit warmup: the row count has period ROWS and the probe can stop
    // within one cycle, before the widest frame is recorded. Four cycles.
    // Budget 4, not 0: `PaintSnapArena::maybe_compact` / `diff_changed_leg`
    // allocate when the damage snapshot arena compacts, periodically.
    Audit::new()
        .warmup(4 * ROWS as usize)
        .budget(4)
        .run(move |ui| {
            step += 1;
            rows(ui, Sizing::FILL, 0..1 + step % ROWS, 20.0);
        });
}

/// Empty rows `height` tall, one per id, in a column `width` wide.
fn rows(ui: &mut Ui, width: Sizing, ids: Range<u32>, height: f32) {
    Panel::vstack()
        .id_salt("rows")
        .size((width, Sizing::FILL))
        .show(ui, |ui| {
            for row in ids {
                Panel::hstack()
                    .id_salt(row)
                    .size((Sizing::FILL, Sizing::fixed(height)))
                    .show(ui, |_ui| {});
            }
        });
}

/// Text whose content changes every frame (a clock, an FPS readout). Each
/// frame mints a text hash nothing asks for again, which defeats a
/// single-deadline cache; hence the shaped-buffer cache's probation tier.
/// Interning new bytes must write them somewhere, so the guard is flatness.
#[test]
fn changing_label_text_stays_flat() {
    let mut step = 0u32;
    // Formatted into a retained buffer: a `String` per label would be the
    // fixture allocating.
    let mut buf = String::with_capacity(64);
    // Explicit warmup: eight shaped buffers are inserted per frame and expire
    // four frames later, so caches keep resizing past the probe's quiet frames.
    // Eight new-text runs at the ~10-per-run cosmic floor: the highest budget.
    Audit::new().warmup(128).budget(112).run(move |ui| {
        step += 1;
        Panel::vstack()
            .id_salt("ticker-root")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for row in 0..ROWS {
                    buf.clear();
                    write!(buf, "row {row} tick {step}").expect("writing to a String");
                    Text::new(buf.as_str()).id_salt(row).show(ui);
                }
            });
    });
}

/// Text re-interned every frame: a handle is valid for the pass that minted
/// it, so a steady scene interns the same bytes into the same arena each
/// frame. Budget zero because `clear` keeps the arena's capacity.
#[test]
fn reinterned_text_alloc_free() {
    Audit::new().warmup(8).run(move |ui| {
        let label = ui.intern("re-interned every frame");
        hint::black_box(label);
        Panel::vstack()
            .id_salt("intern-per-frame")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |_ui| {});
    });
}

/// One frame with many widgets, then a small scene whose layout moves every
/// frame, so every cascade run is a full rebuild. The seen-id tables swap each
/// frame and the spike grows one for good; the cascade's own id table must not
/// reallocate to follow them.
#[test]
fn a_widget_count_spike_leaves_full_rebuilds_alloc_free() {
    let mut step = 0u32;
    Audit::new().warmup(16).run(move |ui| {
        step += 1;
        let count = if step == 1 { 256 } else { ROWS };
        let width = Sizing::fixed(200.0 + (step % 32) as f32);
        rows(ui, width, 0..count, 4.0);
    });
}
