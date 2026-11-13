//! What the table gives back: rows idle past an expiry window above the soft rows, the capacity
//! once a quarter or less is held, and in a purge every row the frame did not draw.

use crate::common::counters::CounterSet;
use crate::renderer::row_table::tests::support::{baked, fill_rows, ramp, runs, table};
use crate::renderer::row_table::*;

/// A table of 4 rows, soft at 8, that 20 ramps grew to 32 in frame 0, its frames registering ramps
/// 0 to 3 alone, keeps them all through frame `EXPIRY_FRAMES`; at the next, the decay takes the 16
/// others, idle past the window: the walk passes rows 1 to 4, then takes row 5 sixteen times, the
/// last row moving into its place 15 times and the 16th the last itself, 20 steps of the 20 rows.
/// Four held rows, a quarter or less of 32, drop the capacity to 16, the smallest rung of 4 that
/// holds twice the five rows; ramps 0 to 3 keep rows 1 to 4, and the flush owes no row, as the
/// rows moved left with the rest.
#[test]
fn a_decay_takes_rows_idle_past_the_window_and_drops_the_capacity() {
    let mut table = table(4, 8, 32);
    let rows = fill_rows(&mut table, 20);
    assert_eq!(table.capacity(), 32);
    runs(&mut table);
    let before = table.counters.counts();
    for frame in 1..=EXPIRY_FRAMES {
        table.advance_to(frame);
        assert_eq!(fill_rows(&mut table, 4), rows[..4]);
        table.decay();
    }
    assert_eq!((table.live_rows(), table.capacity()), (21, 32));
    table.advance_to(EXPIRY_FRAMES + 1);
    table.decay();
    assert_eq!((table.live_rows(), table.capacity()), (5, 16));
    for i in 0..4 {
        assert_eq!(table.resident_row(&ramp(i)), Some(rows[i as usize]));
    }
    assert_eq!(runs(&mut table), []);
    let delta = table.counters.counts() - before;
    assert_eq!((delta.expiries, delta.moves, delta.shrinks), (16, 15, 1));
    assert_eq!(table.index_len(), 4);
}

/// A decay moves at most `AGE_EVICTIONS_MAX` rows a frame: 99 idle ramps of 100 in a table of
/// 128 go 32 a decay, the hand passing the one live row once, 69, then 37, then 5 rows held; the
/// third drops the capacity to 16, the rung that holds twice five rows, and the fourth takes the
/// last three, two moves and the last, and drops it to 4, where the soft rows of 8 stop the decay.
/// Each moved row is the one live ramp's or an idle one's, baked again where it lands.
#[test]
fn a_decay_moves_at_most_its_bound_a_frame() {
    let mut table = table(4, 8, 128);
    fill_rows(&mut table, 100);
    runs(&mut table);
    let mut held = Vec::new();
    for frame in EXPIRY_FRAMES + 1..EXPIRY_FRAMES + 6 {
        table.advance_to(frame);
        table.register(&ramp(0));
        table.decay();
        held.push((table.live_rows(), table.capacity()));
    }
    assert_eq!(held, [(69, 128), (37, 128), (5, 16), (2, 4), (2, 4)]);
    assert_eq!(table.resident_row(&ramp(0)), Some(LutRow(1)));
    assert_eq!(table.counters.counts().moves, 32 + 32 + 32 + 2);
}

/// Under its soft rows a table ages nothing, however idle: a table of 4, soft at 8, holding 7
/// ramps from frame 0, keeps them all two windows on.
#[test]
fn a_table_under_its_soft_rows_never_decays() {
    let mut table = table(4, 8, 32);
    fill_rows(&mut table, 7);
    assert_eq!(table.capacity(), 8);
    for frame in 1..=2 * EXPIRY_FRAMES {
        table.advance_to(frame);
        table.decay();
    }
    assert_eq!((table.live_rows(), table.capacity()), (8, 8));
    assert_eq!(table.counters.counts().expiries, 0);
}

/// A purge takes every row the frame did not draw at once and compacts the rest: 20 ramps from
/// frame 0 in 32 rows, of which frame 3 draws the last five, at rows 16 to 20. Row 1 goes and
/// takes ramp 19, row 2 ramp 18, on to row 5 ramp 15, the rest go with no move; five held rows
/// drop the capacity to 16, the first rows, and the flush owes rows 1 to 5, baked from their new
/// ramps.
#[test]
fn a_purge_takes_every_undrawn_row_and_compacts_the_rest() {
    let mut table = RowTable::default();
    fill_rows(&mut table, 20);
    assert_eq!(table.capacity(), 32);
    runs(&mut table);
    table.advance_to(3);
    for i in 15..20 {
        table.register(&ramp(i));
    }
    table.purge();
    assert_eq!((table.live_rows(), table.capacity()), (6, FIRST_ROWS));
    for (row, i) in (1..).zip((15..20).rev()) {
        assert_eq!(table.resident_row(&ramp(i)), Some(LutRow(row)), "ramp {i}");
        assert_eq!(table.row_texels(row), baked(&ramp(i)));
    }
    assert_eq!(runs(&mut table), [(1, 6)]);
    assert_eq!(table.index_len(), 5);
}
