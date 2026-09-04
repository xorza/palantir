//! What a full atlas does: grow, evict LRU, or fall back at the cap.

use crate::common::counters::CounterSet;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::renderer::gradient_atlas::tests::support::{
    assert_real_row, distinct_grad, fresh_row, register_for,
};
use crate::renderer::gradient_atlas::*;
use std::collections::HashSet;

/// Filling all 255 real slots then registering one more (after a
/// `flush`, i.e. in the next epoch) evicts the LRU row in
/// 1..INITIAL_ATLAS_ROWS — never row 0 (magenta fallback). The new gradient
/// ends up in the evicted slot; the previously resident row's
/// content hash is gone, while a surviving gradient re-registers
/// onto its exact original row (hit path).
#[test]
fn register_full_atlas_evicts_lru_and_preserves_row_zero() {
    let mut atlas = CpuGradientAtlas::default();
    let mut filled_rows: Vec<LutRow> = Vec::with_capacity((INITIAL_ATLAS_ROWS - 1) as usize);
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        filled_rows.push(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    // Re-touch every gradient except index 0 so the very first
    // registration's row is unambiguously the LRU.
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    // Epoch boundary: everything above was registered "this frame"
    // and is eviction-exempt until a flush.
    let _ = atlas.flush();
    let lru = filled_rows[0];
    // Push one more distinct gradient → forces eviction.
    let evictions = atlas.counters.counts().evictions;
    let new_row = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(
        atlas.counters.counts().evictions,
        evictions + 1,
        "the newcomer must have displaced a resident, not taken a free row",
    );
    assert_ne!(new_row.0, 0, "row 0 (magenta) must never be evicted");
    assert_eq!(
        new_row, lru,
        "newest registration must land in the LRU slot",
    );
    // A surviving gradient re-registers onto its exact original row.
    let survivor = register_for(&mut atlas, distinct_grad(0.01));
    assert_eq!(
        survivor, filled_rows[1],
        "surviving content must reuse its original row exactly",
    );
    // Row 0 still magenta after eviction.
    let magenta = RgbaF16::from(RgbaF32::new(1.0, 0.0, 1.0, 1.0));
    assert!(atlas.baked[0].iter().all(|&t| t == magenta));
}

/// 255 distinct registrations then a 256th in the SAME epoch grows
/// the atlas: every resident row's `LutRow` id is already captured in
/// this frame's draw payloads, so evicting one would silently paint
/// the wrong gradient. More distinct gradients than the table holds is
/// legal content, so capacity doubles and the overflow gets its own
/// row — no crash, no aliasing.
#[test]
fn full_atlas_same_epoch_overflow_grows() {
    let mut atlas = CpuGradientAtlas::default();
    let mut rows = HashSet::new();
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        rows.insert(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS);

    let overflow = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(
        atlas.capacity(),
        INITIAL_ATLAS_ROWS * 2,
        "a full same-epoch table must double, not evict",
    );
    assert!(
        rows.insert(overflow),
        "row {} aliased a gradient this frame's draws already reference",
        overflow.0,
    );
    assert_real_row(&atlas, overflow);
    // Growth invalidates the backend's texture height, so the whole
    // atlas — not just the new rows — must re-upload.
    let flushed = atlas.flush().expect("growth must dirty the atlas");
    assert_eq!(flushed.first_row, 0);
    assert_eq!(flushed.total_rows, INITIAL_ATLAS_ROWS * 2);
    assert_eq!(
        flushed.bytes.len(),
        (INITIAL_ATLAS_ROWS * 2) as usize * size_of::<LutRowTexels>(),
    );
}

/// The hit path stamps the epoch too: re-registering all 255
/// resident gradients after a flush re-protects every row, so a
/// 256th distinct gradient in that same epoch grows rather than
/// evicting a row whose id this frame's draws already hold. The
/// re-registered gradients keep their original rows across growth.
#[test]
fn full_atlas_all_hit_this_epoch_grows() {
    let mut atlas = CpuGradientAtlas::default();
    let mut original = Vec::new();
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        original.push(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    let _ = atlas.flush();
    // New epoch: every row re-registered via the hit path.
    for (i, row) in original.iter().enumerate() {
        assert_eq!(
            register_for(&mut atlas, distinct_grad(i as f32 * 0.01)),
            *row,
            "hit path must reuse the resident row",
        );
    }
    let overflow = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);
    assert!(
        !original.contains(&overflow),
        "row {} aliased an epoch-protected row",
        overflow.0,
    );
}

/// Growth is bounded by the device's texture-height cap. At the cap a
/// full same-epoch table can neither evict nor grow, so the overflow
/// paints the magenta fallback: loudly wrong for that one gradient,
/// but it neither crashes nor repaints rows the frame's other draws
/// already captured. `max_rows` below the initial capacity is raised
/// to fit, so one doubling is all this atlas gets.
#[test]
fn growth_stops_at_max_rows_and_falls_back() {
    let mut atlas = CpuGradientAtlas::new(INITIAL_ATLAS_ROWS * 2);
    let mut rows = HashSet::new();
    // Fill both the initial capacity and the one doubling available.
    for i in 0..(INITIAL_ATLAS_ROWS * 2 - 1) {
        rows.insert(register_for(&mut atlas, distinct_grad(i as f32 * 0.01)));
    }
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);
    assert_eq!(rows.len(), (INITIAL_ATLAS_ROWS * 2 - 1) as usize);

    let bakes = atlas.counters.counts().bakes;
    let overflow = register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(
        overflow,
        LutRow::FALLBACK,
        "capped atlas must fall back to magenta, not evict a live row",
    );
    assert_eq!(atlas.counters.counts().fallbacks, 1);
    assert_eq!(
        atlas.counters.counts().bakes,
        bakes,
        "a fallback must not bake — there is no row to bake into",
    );
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2, "cap must hold");
    // The fallback row is still magenta — the overflow never baked
    // over it.
    let magenta = RgbaF16::from(RgbaF32::new(1.0, 0.0, 1.0, 1.0));
    assert!(atlas.baked[0].iter().all(|&t| t == magenta));

    // Next epoch: rows are evictable again, so the same gradient gets
    // a real row instead of the fallback.
    let _ = atlas.flush();
    let recovered = register_for(&mut atlas, distinct_grad(9999.0));
    assert_ne!(recovered, LutRow::FALLBACK);
    assert_real_row(&atlas, recovered);
}

/// Rows resident before a growth keep their ids AND their baked
/// content — this frame's draw payloads already hold those ids, so a
/// row moving or being rewritten under them would repaint issued draws.
///
/// Lookup goes through the key → row index, which growth doesn't
/// touch, so re-registering a resident gradient afterwards returns its
/// *original* row — the duplicate bake the open-addressed table used
/// to produce (its probe modulus moved with the capacity) is gone.
#[test]
fn growth_preserves_resident_row_content() {
    let mut atlas = CpuGradientAtlas::default();
    let pinned = distinct_grad(0.0);
    let pinned_row = register_for(&mut atlas, pinned.clone());
    let pinned_texels = atlas.baked[pinned_row.0 as usize];
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    // Same epoch throughout, so this forces growth.
    register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);
    assert_eq!(
        atlas.baked[pinned_row.0 as usize], pinned_texels,
        "growth must not disturb a row this frame's draws reference",
    );
    // Growth leaves the index alone, so this resolves to the original
    // row rather than baking a second copy of the same gradient.
    let after = register_for(&mut atlas, pinned);
    assert_eq!(after, pinned_row, "growth baked a duplicate row");
    assert_eq!(
        atlas.baked[after.0 as usize], pinned_texels,
        "the resident row's texels must survive growth intact",
    );
}

/// Hit-path bumps the row stamp: a gradient registered first, then
/// re-registered after others, must survive eviction even when the
/// table fills.
#[test]
fn register_hit_bumps_stamp_protecting_recent_content() {
    let mut atlas = CpuGradientAtlas::default();
    let pinned = distinct_grad(0.0);
    let pinned_row = register_for(&mut atlas, pinned.clone());
    // Fill 253 more rows.
    for i in 1..(INITIAL_ATLAS_ROWS - 2) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    // Re-touch the pinned gradient so its stamp is now the largest.
    let r = register_for(&mut atlas, pinned);
    assert_eq!(r, pinned_row, "re-register must reuse the same row");
    // Epoch boundary so the eviction below is legal (nothing above
    // is referenced by the "current frame" anymore).
    let _ = atlas.flush();
    // Two more distinct registrations: the second forces eviction.
    // The pinned row's recent stamp must keep it alive.
    register_for(&mut atlas, distinct_grad(1000.0));
    let evicted_row = register_for(&mut atlas, distinct_grad(1001.0));
    assert_ne!(
        evicted_row, pinned_row,
        "recently touched row must not be evicted",
    );
}

/// Evicting a row then re-registering its original content re-bakes
/// into some slot; the row is restored, no panics, atlas remains
/// usable. Pin the round-trip explicitly so a future eviction-bug
/// that loses content silently is caught.
#[test]
fn evicted_content_can_be_re_registered() {
    let mut atlas = CpuGradientAtlas::default();
    let first = distinct_grad(0.0);
    let _ = register_for(&mut atlas, first.clone());
    // Fill, cross the epoch boundary, then force eviction of `first`
    // (oldest stamp).
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    let _ = atlas.flush();
    register_for(&mut atlas, distinct_grad(9999.0));
    // Re-register `first` — must succeed and return a valid row.
    let reborn = register_for(&mut atlas, first);
    assert_real_row(&atlas, reborn);
}

/// The invariant `register_stops` reads eviction off: rows registered
/// this epoch form a head prefix of the MRU list, so checking the tail
/// alone is equivalent to scanning for the oldest unprotected row.
///
/// Built as a genuinely mixed frame — fresh claims, hits on resident
/// rows, and rows left untouched — because the property is only
/// interesting when all three are present. Checked again after a flush
/// (the whole list becomes stale, so a vacuous all-stale prefix) and
/// after a partial re-touch in the new epoch.
#[test]
fn epoch_current_rows_form_an_mru_prefix() {
    let mut atlas = CpuGradientAtlas::default();
    for i in 0..40 {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    let _ = atlas.flush();
    assert!(atlas.epoch_prefix_holds(), "a fresh epoch protects nothing",);

    // New epoch: re-touch some resident rows out of insertion order,
    // claim some fresh ones, leave the rest alone.
    for i in [7, 31, 2, 19] {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    for i in 40..48 {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    for i in [3, 44] {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    assert!(
        atlas.epoch_prefix_holds(),
        "hits and claims must both move their row to the MRU head",
    );

    // 13 distinct rows were registered this epoch — 4 re-touched, 8
    // freshly claimed, then 3 (new) and 44 (already counted, a repeat
    // hit inside the same epoch). Pinning the count keeps the prefix
    // check above from passing vacuously on an empty prefix.
    let protected = (0..48)
        .filter(|i| {
            let g = distinct_grad(*i as f32 * 0.01);
            atlas
                .resident_row(&g.stops, g.interp)
                .is_some_and(|row| atlas.slots[row as usize].epoch == atlas.epoch)
        })
        .count();
    assert_eq!(protected, 13);
}

/// Growth leaves lookup alone: every gradient resident beforehand still
/// resolves to the row it already occupied, so no draw payload issued
/// this frame is repainted and no duplicate row is baked.
#[test]
fn growth_leaves_resident_lookups_on_their_original_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let resident: Vec<LinearGradient> = (0..(INITIAL_ATLAS_ROWS - 1))
        .map(|i| distinct_grad(i as f32 * 0.01))
        .collect();
    let before: Vec<u32> = resident
        .iter()
        .map(|g| register_for(&mut atlas, g.clone()).0)
        .collect();
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS);

    // Same epoch, so the overflow has to grow rather than evict.
    register_for(&mut atlas, distinct_grad(9999.0));
    assert_eq!(atlas.capacity(), INITIAL_ATLAS_ROWS * 2);

    let bakes = atlas.counters.counts().bakes;
    for (g, &row) in resident.iter().zip(&before) {
        assert_eq!(
            atlas.resident_row(&g.stops, g.interp),
            Some(row),
            "growth moved a resident gradient off row {row}",
        );
        assert_eq!(
            register_for(&mut atlas, g.clone()).0,
            row,
            "re-registering after growth baked a duplicate instead of \
             resolving to row {row}",
        );
    }
    assert_eq!(
        atlas.counters.counts().bakes,
        bakes,
        "re-registering after growth baked at all — the open-addressed \
         table used to duplicate here because its probe modulus moved",
    );
    // A duplicate bake would have consumed rows beyond the 255 resident
    // ones plus the overflow.
    assert_eq!(atlas.index_len(), INITIAL_ATLAS_ROWS as usize);
}

/// Eviction takes the outgoing gradient out of the index with its row.
/// Leaving it behind is the failure mode unique to splitting lookup
/// from storage: the stale entry would resolve to a row now holding
/// somebody else's bake, and the evicted gradient would paint the
/// wrong colours instead of re-baking.
#[test]
fn eviction_drops_the_outgoing_key_from_the_index() {
    let mut atlas = CpuGradientAtlas::default();
    let first = distinct_grad(0.0);
    let first_row = register_for(&mut atlas, first.clone()).0;
    for i in 1..(INITIAL_ATLAS_ROWS - 1) {
        register_for(&mut atlas, distinct_grad(i as f32 * 0.01));
    }
    let _ = atlas.flush();

    // `first` is the least-recently-registered, so it is the victim.
    let newcomer = distinct_grad(9999.0);
    assert_eq!(register_for(&mut atlas, newcomer.clone()).0, first_row);
    assert_eq!(
        atlas.resident_row(&first.stops, first.interp),
        None,
        "evicted gradient still resolves to a row",
    );
    assert_eq!(
        atlas.resident_row(&newcomer.stops, newcomer.interp),
        Some(first_row),
    );
    // The table stayed at one entry per occupied row.
    assert_eq!(atlas.index_len(), (INITIAL_ATLAS_ROWS - 1) as usize);

    // Re-registering the evicted content re-bakes it somewhere else,
    // and its texels are the real gradient rather than the newcomer's.
    let _ = atlas.flush();
    let reborn = register_for(&mut atlas, first.clone()).0;
    assert_ne!(reborn, first_row);
    let mut expected = fresh_row();
    bake_stops(&first.stops, first.interp, &mut expected);
    assert_eq!(atlas.baked[reborn as usize], expected);
}
