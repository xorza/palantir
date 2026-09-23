//! Row assignment: reuse, dedup, and the reserved fallback at row zero.

use crate::primitives::brush::gradient::Interp;
use crate::primitives::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::color::RgbaF32;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::renderer::gradient_atlas::tests::support::{
    assert_real_row, distinct_grad, register_for,
};
use crate::renderer::gradient_atlas::*;
use crate::renderer::texture_limit::TextureLimit;
use glam::Vec2;
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
    let magenta = RgbaF16::new(1.0, 0.0, 1.0, 1.0);
    assert!(atlas.baked[0].iter().all(|&t| t == magenta));
}

/// First real `register` goes through the probe path. The atlas
/// is already dirty from magenta init; registering should keep it
/// dirty so the GPU upload includes the new row.
#[test]
fn register_returns_nonzero_row_and_marks_dirty() {
    let mut atlas = CpuGradientAtlas::default();
    let g = distinct_grad(0.1);
    let row = atlas.register(&g.ramp);
    assert_real_row(&atlas, row);
    assert!(atlas.dirty.is_some(), "register must mark atlas dirty");
}

/// Same gradient registered twice returns the same row and does
/// not re-mark dirty after a flush.
#[test]
fn register_same_gradient_twice_reuses_row() {
    let mut atlas = CpuGradientAtlas::default();
    let g = distinct_grad(0.5);
    let r1 = atlas.register(&g.ramp);
    // Flush so subsequent registrations of the same content can
    // be detected as no-ops.
    let _ = atlas.flush();
    let r2 = atlas.register(&g.ramp);
    assert_eq!(r1, r2);
    assert!(
        atlas.dirty.is_none(),
        "re-registering existing content must not dirty",
    );
}

/// Keys differing in the smallest possible way — one stop byte, or
/// only the interpolation space — must land on different rows. The
/// index is keyed on the whole `ColorRamp`, so this is hashbrown's
/// `Eq` doing the work rather than a hand-written confirm; the atlas
/// still owns the claim that nothing *else* distinguishes a bake.
#[test]
fn near_identical_keys_never_share_a_row() {
    let mut atlas = CpuGradientAtlas::default();
    let base = LinearGradient::two_stop(0.0, SrgbaU8::rgb(10, 20, 30).into(), RgbaF32::WHITE);
    let one_byte_off =
        LinearGradient::two_stop(0.0, SrgbaU8::rgb(10, 20, 31).into(), RgbaF32::WHITE);

    let mut rows = HashSet::new();
    for g in [&base, &one_byte_off] {
        for interp in [Interp::Oklab, Interp::Linear] {
            let row = atlas.register(&g.ramp.with_interp(interp));
            assert_real_row(&atlas, row);
            assert!(rows.insert(row), "row {} aliased a distinct key", row.0);
        }
    }
    assert_eq!(rows.len(), 4);

    // And each of the four still resolves back to its own row.
    let first = atlas.register(&base.ramp.with_interp(Interp::Oklab));
    let second = atlas.register(&one_byte_off.ramp.with_interp(Interp::Oklab));
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
        let row = atlas.register(&g.ramp);
        assert!(
            seen.insert(row),
            "row {} reused across distinct gradients",
            row.0,
        );
        assert_real_row(&atlas, row);
    }
    assert_eq!(seen.len(), INITIAL_ATLAS_ROWS as usize - 1);
}

/// The atlas keys on the ramp alone, so a linear gradient, a radial
/// gradient and a bare curve ramp with the same stops and interp share
/// one row. Geometry differs in the shader (per-fragment `t`), but the
/// LUT bake doesn't depend on it.
#[test]
fn register_dedups_across_variants() {
    let mut atlas = CpuGradientAtlas::default();
    let stops = [
        Stop::new(0.0, SrgbaU8::rgb(255, 64, 0).into()),
        Stop::new(1.0, SrgbaU8::rgb(0, 128, 255).into()),
    ];
    let linear = LinearGradient::new(0.3, stops);
    let radial = RadialGradient::new(Vec2::splat(0.5), Vec2::splat(0.5), stops);
    let curve = ColorRamp::new(stops);
    assert_eq!(
        linear.ramp.interp,
        Interp::Oklab,
        "both kinds default to Oklab"
    );
    assert_eq!(radial.ramp.interp, Interp::Oklab);
    let r_linear = atlas.register(&linear.ramp);
    assert_eq!(atlas.register(&radial.ramp), r_linear);
    assert_eq!(atlas.register(&curve), r_linear);
    // Same stops, different interp → different row.
    let r_other_interp = atlas.register(&curve.with_interp(Interp::Linear));
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
