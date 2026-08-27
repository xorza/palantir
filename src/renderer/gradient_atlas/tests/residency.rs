//! Row assignment: reuse, dedup, and the reserved fallback at row zero.

use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::color::ColorU8;
use crate::renderer::gradient_atlas::tests::support::{
    assert_real_row, distinct_grad, register_for,
};
use crate::renderer::gradient_atlas::*;
use crate::renderer::texture_limit::TextureLimit;
use std::collections::HashSet;

/// Row 0 is reserved magenta. Created at construction; dirty list
/// flags it so the first frame's GPU upload paints the fallback row.
/// First real registration goes to row 1 (or wherever its hash lands
/// in 1..INITIAL_ATLAS_ROWS).
#[test]
fn row_zero_reserved_as_magenta_fallback() {
    let atlas = CpuGradientAtlas::default();
    // Row 0 is linear (1, 0, 1, 1) across all texels — encodes to
    // #ff00ff on the sRGB framebuffer.
    let magenta = ColorF16::from(Color::linear_rgba(1.0, 0.0, 1.0, 1.0));
    assert!(atlas.baked[0].iter().all(|&t| t == magenta));
}

/// First real `register` goes through the probe path. The atlas
/// is already dirty from magenta init; registering should keep it
/// dirty so the GPU upload includes the new row.
#[test]
fn register_returns_nonzero_row_and_marks_dirty() {
    let mut atlas = CpuGradientAtlas::default();
    let g = distinct_grad(0.1);
    let row = atlas.register_stops(&g.stops, g.interp);
    assert_real_row(&atlas, row);
    assert!(atlas.dirty.is_some(), "register must mark atlas dirty");
}

/// Same gradient registered twice returns the same row and does
/// not re-mark dirty after a flush.
#[test]
fn register_same_gradient_twice_reuses_row() {
    let mut atlas = CpuGradientAtlas::default();
    let g = distinct_grad(0.5);
    let r1 = atlas.register_stops(&g.stops, g.interp);
    // Flush so subsequent registrations of the same content can
    // be detected as no-ops.
    let _ = atlas.flush();
    let r2 = atlas.register_stops(&g.stops, g.interp);
    assert_eq!(r1, r2);
    assert!(
        atlas.dirty.is_none(),
        "re-registering existing content must not dirty",
    );
}

/// Keys differing in the smallest possible way — one stop byte, or
/// only the interpolation space — must land on different rows. The
/// index is keyed on the whole `GradientLutKey`, so this is hashbrown's
/// `Eq` doing the work rather than a hand-written confirm; the atlas
/// still owns the claim that nothing *else* distinguishes a bake.
#[test]
fn near_identical_keys_never_share_a_row() {
    let mut atlas = CpuGradientAtlas::default();
    let base = LinearGradient::two_stop(0.0, ColorU8::rgb(10, 20, 30), ColorU8::WHITE);
    let one_byte_off = LinearGradient::two_stop(0.0, ColorU8::rgb(10, 20, 31), ColorU8::WHITE);

    let mut rows = HashSet::new();
    for g in [&base, &one_byte_off] {
        for interp in [Interp::Oklab, Interp::Linear] {
            let row = atlas.register_stops(&g.stops, interp);
            assert_real_row(&atlas, row);
            assert!(rows.insert(row), "row {} aliased a distinct key", row.0);
        }
    }
    assert_eq!(rows.len(), 4);

    // And each of the four still resolves back to its own row.
    let first = atlas.register_stops(&base.stops, Interp::Oklab);
    let second = atlas.register_stops(&one_byte_off.stops, Interp::Oklab);
    assert_ne!(first, second);
}

/// Distinct gradients get distinct rows; both leave the atlas
/// dirty for upload.
#[test]
fn register_distinct_gradients_get_distinct_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let _ = atlas.flush();
    let ra = register_for(&mut atlas, distinct_grad(0.1));
    let rb = register_for(&mut atlas, distinct_grad(0.2));
    assert_ne!(ra, rb);
    assert!(atlas.dirty.is_some());
}

/// Filling the atlas one distinct gradient at a time hands out every
/// real row exactly once — no key aliases another's row, and no row is
/// skipped, so the whole table is reachable.
#[test]
fn register_many_distinct_gradients_all_unique_rows() {
    let mut atlas = CpuGradientAtlas::default();
    let mut seen = HashSet::new();
    for i in 0..(INITIAL_ATLAS_ROWS - 1) {
        let g = distinct_grad(i as f32 * 0.01);
        let row = atlas.register_stops(&g.stops, g.interp);
        assert!(
            seen.insert(row),
            "row {} reused across distinct gradients",
            row.0,
        );
        assert_real_row(&atlas, row);
    }
    assert_eq!(seen.len(), INITIAL_ATLAS_ROWS as usize - 1);
}

/// (stops, interp) keying is variant-agnostic: a linear and a
/// radial gradient with matching stops + interp share one atlas
/// row. Geometry differs in the shader (per-fragment `t`), but the
/// LUT bake doesn't depend on it.
#[test]
fn register_stops_dedups_across_variants() {
    let mut atlas = CpuGradientAtlas::default();
    let stops = GradientStops::new([
        Stop::new(0.0, ColorU8::rgb(255, 64, 0)),
        Stop::new(1.0, ColorU8::rgb(0, 128, 255)),
    ]);
    let r_linear = atlas.register_stops(&stops, Interp::Oklab);
    let r_radial = atlas.register_stops(&stops, Interp::Oklab);
    assert_eq!(r_linear, r_radial);
    // Same stops, different interp → different row.
    let r_other_interp = atlas.register_stops(&stops, Interp::Linear);
    assert_ne!(r_linear, r_other_interp);
}

/// The row ceiling is the *policy* cap, not the device's texture
/// limit: growth never reverses, so a 16384-row adapter would let one
/// pathological frame pin 32 MB for the life of the process.
#[test]
fn shared_atlas_clamps_device_limit_to_the_policy_cap() {
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    use std::num::NonZeroU32;

    let huge = SharedGradientAtlas::new(TextureLimit::from_device(NonZeroU32::new(16384).unwrap()));
    assert_eq!(huge.max_rows(), MAX_ATLAS_ROWS);
    // A device below the cap still binds.
    let small = SharedGradientAtlas::new(TextureLimit::from_device(NonZeroU32::new(1024).unwrap()));
    assert_eq!(small.max_rows(), 1024);
    // Deviceless keeps the conservative downlevel fallback.
    assert_eq!(
        SharedGradientAtlas::new(TextureLimit::default()).max_rows(),
        DEFAULT_MAX_ATLAS_ROWS,
    );
}
