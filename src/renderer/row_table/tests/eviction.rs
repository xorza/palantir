//! What a full table does: grow, evict LRU, or fall back at the cap.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use crate::common::counters::CounterSet;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::renderer::row_table::tests::support::{
    assert_real_row, distinct_grad, fill_rows, fresh_row,
};
use crate::renderer::row_table::*;
use std::collections::HashSet;

/// Filling all 255 real slots then registering one more in the next epoch evicts
/// the LRU row in 1..INITIAL_ROWS, never row 0 (magenta fallback); a
/// surviving gradient re-registers onto its exact original row.
#[test]
fn register_full_atlas_evicts_lru_and_preserves_row_zero() {
    let mut table = RowTable::default();
    let filled_rows = fill_rows(&mut table, INITIAL_ROWS - 1);
    // Re-touch all but index 0 so the first registration's row is the LRU.
    for i in 1..(INITIAL_ROWS - 1) {
        table.register(&distinct_grad(i).ramp);
    }
    let _ = table.flush();
    let lru = filled_rows[0];
    let evictions = table.counters.counts().evictions;
    let new_row = table.register(&distinct_grad(999900).ramp);
    assert_eq!(
        table.counters.counts().evictions,
        evictions + 1,
        "the newcomer must have displaced a resident, not taken a free row",
    );
    assert_ne!(new_row.0, 0, "row 0 (magenta) must never be evicted");
    assert_eq!(
        new_row, lru,
        "newest registration must land in the LRU slot",
    );
    let survivor = table.register(&distinct_grad(1).ramp);
    assert_eq!(
        survivor, filled_rows[1],
        "surviving content must reuse its original row exactly",
    );
    let magenta = RgbaF16::new(1.0, 0.0, 1.0, 1.0);
    assert!(table.row_texels(0).iter().all(|&t| t == magenta));
}

/// A 256th distinct registration in the SAME epoch grows the table: every resident
/// `LutRow` id is already captured in this frame's draw payloads, so evicting one
/// would paint the wrong gradient. The flush owes the rows baked, no more: the magenta
/// row 0, the 255 filled and the overflow at 256, `0..257`, as the backend copies the rest
/// into its taller texture.
#[test]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "a flush of one run is a one-run slice"
)]
fn full_atlas_same_epoch_overflow_grows() {
    let mut table = RowTable::default();
    let mut rows: HashSet<_> = fill_rows(&mut table, INITIAL_ROWS - 1)
        .into_iter()
        .collect();
    assert_eq!(table.capacity(), INITIAL_ROWS);

    let overflow = table.register(&distinct_grad(999900).ramp);
    assert_eq!(
        table.capacity(),
        INITIAL_ROWS * 2,
        "a full same-epoch table must double, not evict",
    );
    assert!(
        rows.insert(overflow),
        "row {} aliased a gradient this frame's draws already reference",
        overflow.0,
    );
    assert_real_row(&table, overflow);
    let flushed = table.flush().expect("growth must dirty the table");
    assert_eq!(flushed.runs, [0..INITIAL_ROWS + 1]);
    assert_eq!(flushed.total_rows, INITIAL_ROWS * 2);
}

/// The hit path stamps the epoch too: re-registering all 255 resident gradients
/// after a flush re-protects every row, so a 256th grows rather than evicting.
#[test]
fn full_atlas_all_hit_this_epoch_grows() {
    let mut table = RowTable::default();
    let original = fill_rows(&mut table, INITIAL_ROWS - 1);
    let _ = table.flush();
    for (i, row) in original.iter().enumerate() {
        assert_eq!(
            table.register(&distinct_grad(i as u32).ramp),
            *row,
            "hit path must reuse the resident row",
        );
    }
    let overflow = table.register(&distinct_grad(999900).ramp);
    assert_eq!(table.capacity(), INITIAL_ROWS * 2);
    assert!(
        !original.contains(&overflow),
        "row {} aliased an epoch-protected row",
        overflow.0,
    );
}

/// Growth is bounded by the device's texture-height cap. At the cap a full
/// same-epoch table can neither evict nor grow, so the overflow paints the magenta
/// fallback: no crash and no repaint of captured rows. `max_rows` below the initial
/// capacity is raised to fit.
#[test]
fn growth_stops_at_max_rows_and_falls_back() {
    let mut table = RowTable::new(INITIAL_ROWS * 2);
    let rows: HashSet<_> = fill_rows(&mut table, INITIAL_ROWS * 2 - 1)
        .into_iter()
        .collect();
    assert_eq!(table.capacity(), INITIAL_ROWS * 2);
    assert_eq!(rows.len(), (INITIAL_ROWS * 2 - 1) as usize);

    let bakes = table.counters.counts().bakes;
    let overflow = table.register(&distinct_grad(999900).ramp);
    assert_eq!(
        overflow,
        LutRow::FALLBACK,
        "capped table must fall back to magenta, not evict a live row",
    );
    assert_eq!(table.counters.counts().fallbacks, 1);
    assert_eq!(
        table.counters.counts().bakes,
        bakes,
        "a fallback must not bake — there is no row to bake into",
    );
    assert_eq!(table.capacity(), INITIAL_ROWS * 2, "cap must hold");
    let magenta = RgbaF16::new(1.0, 0.0, 1.0, 1.0);
    assert!(table.row_texels(0).iter().all(|&t| t == magenta));

    let _ = table.flush();
    let recovered = table.register(&distinct_grad(999900).ramp);
    assert_ne!(recovered, LutRow::FALLBACK);
    assert_real_row(&table, recovered);
}

/// Rows resident before a growth keep their ids AND baked content, since this
/// frame's draw payloads hold those ids; lookup goes through the key to row index,
/// which growth does not touch.
#[test]
fn growth_preserves_resident_row_content() {
    let mut table = RowTable::default();
    let pinned = distinct_grad(0);
    let pinned_row = table.register(&pinned.clone().ramp);
    let pinned_texels = table.row_texels(pinned_row.0);
    for i in 1..(INITIAL_ROWS - 1) {
        table.register(&distinct_grad(i).ramp);
    }
    table.register(&distinct_grad(999900).ramp);
    assert_eq!(table.capacity(), INITIAL_ROWS * 2);
    assert_eq!(
        table.row_texels(pinned_row.0),
        pinned_texels,
        "growth must not disturb a row this frame's draws reference",
    );
    let after = table.register(&pinned.ramp);
    assert_eq!(after, pinned_row, "growth baked a duplicate row");
    assert_eq!(
        table.row_texels(after.0),
        pinned_texels,
        "the resident row's texels must survive growth intact",
    );
}

/// A hit bumps the row stamp: a gradient re-registered after others must survive
/// eviction when the table fills.
#[test]
fn register_hit_bumps_stamp_protecting_recent_content() {
    let mut table = RowTable::default();
    let pinned = distinct_grad(0);
    let pinned_row = table.register(&pinned.clone().ramp);
    for i in 1..(INITIAL_ROWS - 2) {
        table.register(&distinct_grad(i).ramp);
    }
    let r = table.register(&pinned.ramp);
    assert_eq!(r, pinned_row, "re-register must reuse the same row");
    let _ = table.flush();
    table.register(&distinct_grad(100000).ramp);
    let evicted_row = table.register(&distinct_grad(100100).ramp);
    assert_ne!(
        evicted_row, pinned_row,
        "recently touched row must not be evicted",
    );
}

/// Evicting a row then re-registering its content re-bakes into some slot and
/// restores the row.
#[test]
fn evicted_content_can_be_re_registered() {
    let mut table = RowTable::default();
    let first = distinct_grad(0);
    let _ = table.register(&first.clone().ramp);
    for i in 1..(INITIAL_ROWS - 1) {
        table.register(&distinct_grad(i).ramp);
    }
    let _ = table.flush();
    table.register(&distinct_grad(999900).ramp);
    let reborn = table.register(&first.ramp);
    assert_real_row(&table, reborn);
}

/// `register` reads eviction off an invariant: rows registered this epoch form a
/// head prefix of the MRU list, so checking the tail alone finds the oldest
/// unprotected row. Built as a mixed frame (fresh claims, hits, untouched rows),
/// checked again after a flush (an all-stale prefix) and after a partial re-touch.
#[test]
fn epoch_current_rows_form_an_mru_prefix() {
    let mut table = RowTable::default();
    fill_rows(&mut table, 40);
    let _ = table.flush();
    assert!(table.epoch_prefix_holds(), "a fresh epoch protects nothing");

    for i in [7, 31, 2, 19] {
        table.register(&distinct_grad(i as u32).ramp);
    }
    for i in 40..48 {
        table.register(&distinct_grad(i as u32).ramp);
    }
    for i in [3, 44] {
        table.register(&distinct_grad(i as u32).ramp);
    }
    assert!(
        table.epoch_prefix_holds(),
        "hits and claims must both move their row to the MRU head",
    );

    // 13 distinct rows registered this epoch: 4 re-touched, 8 fresh, then 3 (new)
    // and 44 (a repeat hit). The count stops the prefix check passing vacuously on
    // an empty prefix.
    let protected = (0..48)
        .filter(|i| {
            let g = distinct_grad(*i as u32);
            table
                .resident_row(&g.ramp)
                .is_some_and(|row| table.slots[row as usize].epoch == table.epoch)
        })
        .count();
    assert_eq!(protected, 13);
}

/// Growth leaves lookup alone: every resident gradient still resolves to its row,
/// so no issued draw is repainted and no duplicate is baked.
#[test]
fn growth_leaves_resident_lookups_on_their_original_rows() {
    let mut table = RowTable::default();
    let resident: Vec<LinearGradient> = (0..(INITIAL_ROWS - 1)).map(distinct_grad).collect();
    let before: Vec<u32> = resident
        .iter()
        .map(|g| table.register(&g.clone().ramp).0)
        .collect();
    assert_eq!(table.capacity(), INITIAL_ROWS);

    table.register(&distinct_grad(999900).ramp);
    assert_eq!(table.capacity(), INITIAL_ROWS * 2);

    let bakes = table.counters.counts().bakes;
    for (g, &row) in resident.iter().zip(&before) {
        assert_eq!(
            table.resident_row(&g.ramp),
            Some(row),
            "growth moved a resident gradient off row {row}",
        );
        assert_eq!(
            table.register(&g.clone().ramp).0,
            row,
            "re-registering after growth baked a duplicate instead of \
             resolving to row {row}",
        );
    }
    assert_eq!(
        table.counters.counts().bakes,
        bakes,
        "re-registering after growth baked at all — the open-addressed \
         table used to duplicate here because its probe modulus moved",
    );
    assert_eq!(table.index_len(), INITIAL_ROWS as usize);
}

/// Eviction takes the outgoing gradient out of the index with its row; a stale
/// entry would resolve to a row holding somebody else's bake.
#[test]
fn eviction_drops_the_outgoing_key_from_the_index() {
    let mut table = RowTable::default();
    let first = distinct_grad(0);
    let first_row = table.register(&first.ramp).0;
    for i in 1..(INITIAL_ROWS - 1) {
        table.register(&distinct_grad(i).ramp);
    }
    let _ = table.flush();

    let newcomer = distinct_grad(999900);
    assert_eq!(table.register(&newcomer.ramp).0, first_row);
    assert_eq!(
        table.resident_row(&first.ramp),
        None,
        "evicted gradient still resolves to a row",
    );
    assert_eq!(table.resident_row(&newcomer.ramp), Some(first_row));
    assert_eq!(table.index_len(), (INITIAL_ROWS - 1) as usize);

    let _ = table.flush();
    let reborn = table.register(&first.ramp).0;
    assert_ne!(reborn, first_row);
    let mut expected = fresh_row();
    bake::row(&first.ramp, &mut expected);
    assert_eq!(table.row_texels(reborn), expected);
}
