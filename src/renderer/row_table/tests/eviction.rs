//! What a full table does: grow under its soft rows, evict a row idle past the grace over them,
//! grow to its ceiling, or fall back.

use crate::common::counters::CounterSet;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::renderer::row_table::tests::support::{
    assert_real_row, baked, distinct_grad, fill_rows, magenta, ramp, runs, table,
};
use crate::renderer::row_table::*;
use std::collections::HashSet;

/// One step of a script: the display frame, and the ramps registered in it.
#[derive(Debug)]
struct Step {
    frame: u64,
    ramps: Range<u32>,
}

/// What a script ends with: the rows its last step's ramps took, the capacity, and the
/// evictions, growths and fallbacks over it.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    last_rows: Vec<u32>,
    capacity: u32,
    evictions: u32,
    growths: u32,
    fallbacks: u32,
}

fn run(mut table: RowTable, steps: &[Step]) -> Outcome {
    let mut last_rows = Vec::new();
    for step in steps {
        table.advance_to(step.frame);
        last_rows = step
            .ramps
            .clone()
            .map(|i| table.register(&ramp(i)).0)
            .collect();
    }
    let counts = table.counters.counts();
    Outcome {
        last_rows,
        capacity: table.capacity(),
        evictions: counts.evictions,
        growths: counts.growths,
        fallbacks: counts.fallbacks,
    }
}

/// Scripts over tables of 4, 8 and 16 rows, each a frame's registrations in order, against the
/// rows, the capacity and the counts they end with.
///
/// - Two owners in one display frame: a's three ramps fill rows 1 to 3 of 4, b's three take a
///   doubled table's 4 to 6, as no row is idle past the grace, and a's next frame hits all three.
///   The flush-counted epoch evicted a's rows for b's and b's for a's, every frame.
/// - The next frame is within the grace too: b at frame 1 grows the table as well; at frame 2 a's
///   rows, last drawn at 0, are idle past it, and b's 7 to 9 take them in the clock's order.
/// - At the ceiling: 7 ramps fill 8 rows, and the 8th takes the fallback, as does the same ramp
///   at frame 1; at frame 2 it evicts the clock's first row, row 1.
/// - Under the soft rows the table grows rather than evict: the 4th ramp, at frame 5, finds rows
///   1 to 3 idle but takes row 4 of a doubled table, and the next eight a table doubled again; at
///   the soft rows, all 16 full, a new ramp evicts row 1.
/// - A dropped build: ramps registered at frame 5, one hit again under another window's build that
///   names frame 4, stay at 5, in the grace through frame 6, where a new ramp takes the fallback of
///   a table at its ceiling; at 7 it evicts row 1.
/// - A ceiling no power of two: a table of 4 rows capped at 12 doubles to 8, then to 12, and its
///   12th ramp takes the fallback.
#[test]
fn a_full_table_grows_evicts_or_falls_back_by_script() {
    let step = |frame, ramps| Step { frame, ramps };
    let cases = [
        (
            "two owners in one display frame",
            table(4, 4, 16),
            vec![step(0, 0..3), step(0, 100..103), step(1, 0..3)],
            Outcome {
                last_rows: vec![1, 2, 3],
                capacity: 8,
                evictions: 0,
                growths: 1,
                fallbacks: 0,
            },
        ),
        (
            "the next frame is within the grace",
            table(4, 4, 16),
            vec![step(0, 0..3), step(1, 100..103), step(2, 103..106)],
            Outcome {
                last_rows: vec![7, 1, 2],
                capacity: 8,
                evictions: 2,
                growths: 1,
                fallbacks: 0,
            },
        ),
        (
            "at the ceiling",
            table(4, 4, 8),
            vec![step(0, 0..7), step(0, 7..8), step(1, 7..8), step(2, 7..8)],
            Outcome {
                last_rows: vec![1],
                capacity: 8,
                evictions: 1,
                growths: 1,
                fallbacks: 2,
            },
        ),
        (
            "under the soft rows",
            table(4, 16, 16),
            vec![
                step(0, 0..3),
                step(5, 3..4),
                step(5, 4..15),
                step(9, 15..16),
            ],
            Outcome {
                last_rows: vec![1],
                capacity: 16,
                evictions: 1,
                growths: 2,
                fallbacks: 0,
            },
        ),
        (
            "a dropped build",
            table(4, 4, 4),
            vec![
                step(5, 0..3),
                step(4, 0..1),
                step(6, 100..101),
                step(7, 100..101),
            ],
            Outcome {
                last_rows: vec![1],
                capacity: 4,
                evictions: 1,
                growths: 0,
                fallbacks: 1,
            },
        ),
        (
            "a ceiling no power of two",
            table(4, 4, 12),
            vec![step(0, 0..11), step(0, 11..12)],
            Outcome {
                last_rows: vec![0],
                capacity: 12,
                evictions: 0,
                growths: 2,
                fallbacks: 1,
            },
        ),
    ];
    for (name, table, steps, want) in cases {
        assert_eq!(run(table, &steps), want, "{name}");
    }
}

/// The latch: a whole turn of the clock that finds nothing idle marks the frame dry, and the rest
/// of the frame's misses take the fallback with no turn, the hand where the turn left it.
#[test]
fn a_dry_frame_turns_the_clock_once() {
    let mut table = table(4, 4, 4);
    for i in 0..3 {
        table.register(&ramp(i));
    }
    table.advance_to(2);
    table.register(&ramp(0));
    assert_eq!(
        table.register(&ramp(100)),
        LutRow(2),
        "the clock's first idle row"
    );
    assert_eq!(table.hand, 2, "past the victim");
    table.register(&ramp(2));
    assert_eq!(table.register(&ramp(101)), LutRow::FALLBACK);
    assert_eq!(table.dry_frame, Some(2));
    let hand = table.hand;
    assert_eq!(table.register(&ramp(102)), LutRow::FALLBACK);
    assert_eq!(table.hand, hand, "no second turn");
    table.advance_to(4);
    assert_eq!(
        table.register(&ramp(102)),
        LutRow(3),
        "a new frame turns again"
    );
}

/// A table full at its soft rows, every row drawn at frame 0, takes at frame 2 the one row no later
/// frame drew, never row 0; the evicted ramp leaves the index with its row; a survivor keeps its
/// row; and the evicted ramp, registered again, bakes into another row.
#[test]
fn an_eviction_takes_the_idle_row_and_its_index_entry() {
    let mut table = RowTable::default();
    let rows = fill_rows(&mut table, SOFT_ROWS - 1);
    table.advance_to(2);
    for i in 1..SOFT_ROWS - 1 {
        table.register(&ramp(i));
    }
    let evictions = table.counters.counts().evictions;
    let newcomer = table.register(&ramp(999_900));
    assert_eq!(table.counters.counts().evictions, evictions + 1);
    assert_eq!(newcomer, rows[0], "the one idle row");
    assert_eq!(
        table.resident_row(&ramp(0)),
        None,
        "the evicted ramp left the index"
    );
    assert_eq!(table.resident_row(&ramp(999_900)), Some(newcomer));
    assert_eq!(table.index_len(), (SOFT_ROWS - 1) as usize);
    assert_eq!(
        table.register(&ramp(1)),
        rows[1],
        "a survivor keeps its row"
    );
    assert!(table.row_texels(0).iter().all(|&t| t == magenta()));

    table.advance_to(4);
    let reborn = table.register(&ramp(0));
    assert_ne!(reborn, rows[0]);
    assert_eq!(table.row_texels(reborn.0), baked(&ramp(0)));
}

/// A hit stamps its row: all 255 ramps registered again at frame 2 keep every row from eviction,
/// so a 256th grows the table past its soft rows rather than alias one.
#[test]
fn a_hit_keeps_its_row_from_eviction() {
    let mut table = RowTable::default();
    let original = fill_rows(&mut table, SOFT_ROWS - 1);
    table.advance_to(2);
    for (i, row) in (0..).zip(&original) {
        assert_eq!(table.register(&ramp(i)), *row);
        assert_eq!(table.last_use(*row), 2);
    }
    let overflow = table.register(&ramp(999_900));
    assert_eq!(table.capacity(), SOFT_ROWS * 2);
    assert!(!original.contains(&overflow));
}

/// A 256th ramp in the frame that drew the 255 grows the table past its soft rows: every resident
/// row id is already in this frame's draw payloads. The flush owes the rows baked, no more: the
/// magenta row 0, the 255 filled and the overflow at 256, `0..257`, as the backend copies the rest
/// into its taller texture.
#[test]
fn a_full_table_grows_within_a_frame() {
    let mut table = RowTable::default();
    let mut rows: HashSet<_> = fill_rows(&mut table, SOFT_ROWS - 1).into_iter().collect();
    let overflow = table.register(&ramp(999_900));
    assert_eq!(table.capacity(), SOFT_ROWS * 2);
    assert!(rows.insert(overflow), "row {} aliased", overflow.0);
    assert_real_row(&table, overflow);
    assert_eq!(table.flush().capacity, SOFT_ROWS * 2);
    assert_eq!(runs(&mut table), [], "the flush cleared them");
    let mut fresh = RowTable::default();
    fill_rows(&mut fresh, SOFT_ROWS);
    assert_eq!(runs(&mut fresh), [(0, SOFT_ROWS + 1)]);
}

/// Growth leaves lookup alone: every resident ramp still resolves to its row with the texels it
/// had, so no issued draw is repainted and no duplicate is baked.
#[test]
fn growth_leaves_resident_rows_and_their_texels() {
    let mut table = RowTable::default();
    let resident: Vec<LinearGradient> = (0..(SOFT_ROWS - 1)).map(distinct_grad).collect();
    let before: Vec<u32> = resident.iter().map(|g| table.register(&g.ramp).0).collect();
    let texels = table.row_texels(before[0]);
    table.register(&ramp(999_900));
    assert_eq!(table.capacity(), SOFT_ROWS * 2);
    let bakes = table.counters.counts().bakes;
    for (g, &row) in resident.iter().zip(&before) {
        assert_eq!(table.resident_row(&g.ramp), Some(LutRow(row)));
        assert_eq!(table.register(&g.ramp).0, row);
    }
    assert_eq!(table.counters.counts().bakes, bakes, "no duplicate bake");
    assert_eq!(table.row_texels(before[0]), texels);
    assert_eq!(table.index_len(), SOFT_ROWS as usize);
}

/// The index holds room for two entries a row, so ten thousand registrations a frame apart, each
/// evicting into a table of 16 rows, leave its capacity where the table set it: hashbrown
/// rehashes in place where an eviction's tombstones fill it, as half or more of it is free.
#[test]
fn eviction_churn_never_resizes_the_index() {
    let mut table = table(16, 16, 16);
    let capacity = table.index.capacity();
    for i in 0..10_000 {
        table.advance_to(2 * u64::from(i));
        let row = table.register(&ramp(i));
        assert_real_row(&table, row);
        assert_eq!(table.index.capacity(), capacity, "registration {i}");
    }
    assert_eq!(table.counters.counts().evictions, 10_000 - 15);
}
