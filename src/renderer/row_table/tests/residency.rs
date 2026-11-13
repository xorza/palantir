//! Row assignment: reuse, dedup, and the reserved fallback at row zero.

use crate::primitives::paint::brush::gradient::Interpolation;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::paint::brush::gradient::stops::Stop;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::renderer::row_table::tests::support::{
    assert_real_row, distinct_grad, fill_rows, magenta, runs,
};
use crate::renderer::row_table::*;
use std::collections::HashSet;

/// Row 0 is reserved magenta, created at construction and flagged dirty so the first frame's GPU
/// upload paints the fallback row.
#[test]
fn row_zero_reserved_as_magenta_fallback() {
    let mut table = RowTable::default();
    assert!(table.row_texels(0).iter().all(|&t| t == magenta()));
    assert_eq!(runs(&mut table), [(0, 1)]);
}

/// Same gradient registered twice returns the same row and does not re-mark dirty after a flush.
/// The memory report counts row 0's fallback alone at first and the one row the ramp holds after,
/// however often it registers.
#[test]
fn register_same_gradient_twice_reuses_row() {
    let mut table = RowTable::default();
    assert_eq!(table.live_rows(), 1, "the fallback alone");
    let g = distinct_grad(50);
    let r1 = table.register(&g.ramp);
    // Flush so later registrations of the same content show as no-ops.
    runs(&mut table);
    let r2 = table.register(&g.ramp);
    assert_eq!(r1, r2);
    assert_eq!(
        runs(&mut table),
        [],
        "re-registering existing content must not dirty"
    );
    assert_eq!(table.live_rows(), 2, "the fallback and the one ramp");
}

/// Keys differing in the smallest possible way (one stop byte, or only the interpolation space) must
/// land on different rows. The index is keyed on the whole `ColorRamp`, so this is hashbrown's `Eq`
/// at work, not a hand-written confirm; the table still owns the claim that nothing *else*
/// distinguishes a bake.
#[test]
fn near_identical_keys_never_share_a_row() {
    let mut table = RowTable::default();
    let base = LinearGradient::horizontal([SrgbaU8::rgb(10, 20, 30).into(), RgbaF32::WHITE]);
    let one_byte_off =
        LinearGradient::horizontal([SrgbaU8::rgb(10, 20, 31).into(), RgbaF32::WHITE]);

    let mut rows = HashSet::new();
    for g in [&base, &one_byte_off] {
        for interpolation in [Interpolation::Oklab, Interpolation::Linear] {
            let row = table.register(&g.ramp.with_interpolation(interpolation));
            assert_real_row(&table, row);
            assert!(rows.insert(row), "row {} aliased a distinct key", row.0);
        }
    }
    assert_eq!(rows.len(), 4);

    // And each of the four still resolves back to its own row.
    let first = table.register(&base.ramp.with_interpolation(Interpolation::Oklab));
    let second = table.register(&one_byte_off.ramp.with_interpolation(Interpolation::Oklab));
    assert_ne!(first, second);
}

/// Filling the table one distinct gradient at a time hands out every real row exactly once: no key
/// aliases another's row and none is skipped, so the whole table is reachable. Each registration
/// marks its row dirty and magenta row 0 still waits from construction, so the one dirty run is
/// every row.
#[test]
fn register_many_distinct_gradients_all_unique_rows() {
    let mut table = RowTable::default();
    let mut seen = HashSet::new();
    for row in fill_rows(&mut table, FIRST_ROWS - 1) {
        assert!(
            seen.insert(row),
            "row {} reused across distinct gradients",
            row.0,
        );
        assert_real_row(&table, row);
    }
    assert_eq!(seen.len(), FIRST_ROWS as usize - 1);
    assert_eq!(table.capacity(), FIRST_ROWS);
    assert_eq!(runs(&mut table), [(0, FIRST_ROWS)]);
}

/// The table keys on the ramp alone, so a linear gradient, a radial gradient and a bare curve ramp
/// with the same stops and interpolation share one row: geometry differs in the shader, but the LUT
/// bake doesn't depend on it.
#[test]
fn register_dedups_across_variants() {
    let mut table = RowTable::default();
    let stops = [
        Stop::new(0.0, SrgbaU8::rgb(255, 64, 0)),
        Stop::new(1.0, SrgbaU8::rgb(0, 128, 255)),
    ];
    let linear = LinearGradient::new(0.3, stops);
    let radial = RadialGradient::centered(stops);
    let curve = ColorRamp::new(stops);
    assert_eq!(
        linear.ramp.interpolation,
        Interpolation::Oklab,
        "both kinds default to Oklab"
    );
    assert_eq!(radial.ramp.interpolation, Interpolation::Oklab);
    let r_linear = table.register(&linear.ramp);
    assert_eq!(table.register(&radial.ramp), r_linear);
    assert_eq!(table.register(&curve), r_linear);
    // Same stops, different interpolation → different row.
    let r_other_interp = table.register(&curve.with_interpolation(Interpolation::Linear));
    assert_ne!(r_linear, r_other_interp);
}

/// The row ceiling is the *policy* cap, not the device's texture limit, so a 16384-row adapter
/// lets no pathological frame take 32 MiB.
#[test]
fn a_table_for_a_device_clamps_its_limit_to_the_policy_cap() {
    let max_rows = |edge| RowTable::new(RowTableConfig::of_device(edge)).max_rows();
    assert_eq!(max_rows(16384), MAX_ROWS);
    // A device below the cap still binds.
    assert_eq!(max_rows(1024), 1024);
    // A device below the soft rows still holds them.
    assert_eq!(max_rows(64), SOFT_ROWS);
    // A table no device holds keeps the conservative downlevel fallback.
    assert_eq!(RowTable::default().max_rows(), DEFAULT_MAX_ROWS);
}
