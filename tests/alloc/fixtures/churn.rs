//! Fixtures whose scene *changes* every frame.
//!
//! Every other fixture in this suite paints the same tree over and over,
//! which measures the steady-state path and nothing else. But the caches
//! this crate is built around — the layout measure cache, the reuse
//! rows, the shaped-buffer cache, the encoded-run cache, the glyph atlas
//! — all short-circuit on a still frame. A scene that never changes
//! never reaches their insert, supersede, or expiry paths, so an
//! allocation introduced there would sail past the whole audit.
//!
//! These drive the four shapes that do reach them: a width drag, text
//! whose content changes, rows entering and leaving the tree, and a
//! scale ramp.
//!
//! Budgets here are not all zero, and that is the measurement talking
//! rather than a concession. Every allocation these fixtures see traces
//! into `cosmic_text`'s `set_text` / `shape_until_scroll` — reshaping
//! genuinely new content builds line, shape and layout runs, and roughly
//! ten allocations per newly-shaped run is the floor this shaper has.
//! Palantir's own scratch (`truncate_scratch`, `break_scratch`,
//! `logical_order`, the recycle pool, the expiry wheels) stays quiet
//! throughout.
//!
//! So what a budget pins here is not zero but *flatness*: the audit
//! checks all 64 frames individually, and a cost that grew with how long
//! the gesture had run — the failure every one of these caches exists to
//! prevent — would blow a fixed ceiling long before the window closed.
//! Tighten a number when a change makes it smaller; a number that has to
//! *rise* is the regression this file exists to catch.

use crate::harness::Audit;
use palantir::{Configure, Panel, Sizing, Text, TextWrap};
use std::fmt::Write as _;

/// Labels per churning fixture — enough that a per-run leak shows up as
/// a multiple rather than as noise.
const ROWS: u32 = 8;

/// A resize drag: the committed width moves every frame, so every text
/// run resolves to a fresh bounded key, supersedes the one it replaces,
/// and mints a shaped buffer that nothing will ask for again.
///
/// This is the workload `cosmic::PROBATION_KEEP_FRAMES` exists for, and
/// the one where a per-frame allocation would compound — a drag runs for
/// hundreds of frames.
#[test]
fn width_drag_stays_flat() {
    let mut step = 0u32;
    // One shaped buffer per run, all of it inside cosmic.
    Audit::new().text().budget(16).run(move |ui| {
        // A whole pixel per frame, which is what a drag commits after
        // the wrap width is quantized.
        let width = 240.0 + (step % 64) as f32;
        step += 1;
        Panel::vstack()
            .id_salt("drag-root")
            .size((Sizing::fixed(width), Sizing::FILL))
            .show(ui, |ui| {
                for row in 0..ROWS {
                    Text::new("a label long enough to need wrapping at this width")
                        .id_salt(row)
                        .text_wrap(TextWrap::Wrap)
                        .show(ui);
                }
            });
    });
}

/// The same drag against a truncating policy, which takes the other
/// branch: `measure_truncated` re-cuts against the cached unbounded
/// probe and reshapes only the prefix, through retained scratch.
#[test]
fn ellipsis_width_drag_stays_flat() {
    let mut step = 0u32;
    // Lands below the wrapping drag: the cut reshapes only the
    // prefix rather than the whole run.
    Audit::new().text().budget(16).run(move |ui| {
        let width = 180.0 + (step % 64) as f32;
        step += 1;
        Panel::vstack()
            .id_salt("ellipsis-root")
            .size((Sizing::fixed(width), Sizing::FILL))
            .show(ui, |ui| {
                for row in 0..ROWS {
                    Text::new("a label far too long for the column it sits in")
                        .id_salt(row)
                        .text_wrap(TextWrap::Ellipsis)
                        .show(ui);
                }
            });
    });
}

/// Rows entering and leaving the tree, the virtualized-list shape: each
/// frame records a different window of ids, so the measure cache's
/// descriptor sequence changes, reuse rows are swept against `removed`,
/// and widget-keyed maps churn.
#[test]
fn scrolling_row_window_alloc_free() {
    let mut first = 0u32;
    Audit::new().run(move |ui| {
        first += 1;
        Panel::vstack()
            .id_salt("scroll-root")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for row in first..first + ROWS {
                    Panel::hstack()
                        .id_salt(row)
                        .size((Sizing::FILL, Sizing::fixed(20.0)))
                        .show(ui, |_ui| {});
                }
            });
    });
}

/// Widget count oscillating rather than sliding: ids are added and
/// removed rather than shifted, so `removed` is non-empty on the shrink
/// frames and every per-widget map takes its eviction path.
#[test]
fn widget_add_remove_stays_flat() {
    let mut step = 0u32;
    // An explicit warmup rather than the probe: the
    // row count oscillates with period ROWS, and the probe stops after
    // two quiet frames - which it can find *within* one cycle, before
    // the widest frame has ever been recorded. The audit window then
    // catches that frame's one-off growth and reads it as a per-frame
    // cost. Four full cycles of warmup is what makes the number honest.
    // Budget 4, not 0, and the reason is a finding rather than a
    // concession: `PaintSnapArena::maybe_compact` / `diff_changed_leg`
    // allocate when the damage engine's snapshot arena compacts, which
    // ratio-based amortization makes periodic rather than per-frame. A
    // still frame never trips it, which is why nothing caught it before.
    Audit::new()
        .warmup(4 * ROWS as usize)
        .budget(4)
        .run(move |ui| {
            step += 1;
            let count = 1 + step % ROWS;
            Panel::vstack()
                .id_salt("add-remove-root")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for row in 0..count {
                        Panel::hstack()
                            .id_salt(row)
                            .size((Sizing::FILL, Sizing::fixed(20.0)))
                            .show(ui, |_ui| {});
                    }
                });
        });
}

/// Text whose *content* changes every frame — a clock, an FPS readout, a
/// scrubbing timecode. Each frame mints a text hash nothing will ask for
/// again, which is the churn that defeats a single-deadline cache and
/// the reason the shaped-buffer cache carries a probation tier.
///
/// Interning genuinely new bytes has to write them somewhere, so this
/// fixture's budget is whatever that costs — the guard is that it stays
/// flat instead of growing with the frame count.
#[test]
fn changing_label_text_stays_flat() {
    let mut step = 0u32;
    // Formatted into a retained buffer, not `format!`: a `String` per
    // label would be the *fixture* allocating, and the audit cannot tell
    // that apart from the engine doing it.
    let mut buf = String::with_capacity(64);
    // An explicit warmup for the same reason as `widget_add_remove`:
    // this scene inserts eight shaped buffers a frame and expires them
    // four frames later, so the caches keep resizing well past the two
    // quiet frames the probe settles for. 128 frames is a long way past
    // where they stop growing.
    //
    // Eight runs whose text is new every frame, at the ~10-per-run
    // cosmic floor — the highest budget here, and rightly so: a fresh
    // text hash cannot reuse anything.
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

/// Text re-interned every frame, which is the contract `InternedStr`
/// now states: a handle is valid for the pass that minted it, so a
/// steady scene interns the same bytes into the same arena frame after
/// frame.
///
/// Budgeted at zero because `clear` keeps the arena's capacity. This
/// used to be two fixtures measuring the opposite question — what it
/// cost to *hold* a handle across frames — back when `TextStore`
/// double-buffered arenas to keep an escaped handle's bytes alive. The
/// one-generation arm rode the spare for free; the two-generation arm
/// allocated a fresh arena every frame, forever, because both were
/// pinned and there was nothing to swap to. Neither question exists now:
/// a handle is `Copy`, owns nothing, and cannot outlive its pass.
#[test]
fn reinterned_text_alloc_free() {
    Audit::new().warmup(8).run(move |ui| {
        let label = ui.intern("re-interned every frame");
        std::hint::black_box(label);
        Panel::vstack()
            .id_salt("intern-per-frame")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |_ui| {});
    });
}
