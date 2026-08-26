//! f16 pack/unpack: the crate's SIMD path against `half`'s scalar one,
//! which is what this module exists to replace.
//!
//! Both directions plus the fused [`f16x4_scaled`], because each is a
//! standing claim in [`super`]'s docs that would otherwise rot silently
//! as codegen and target features move.
//!
//! ## Why there is no `const` arm
//!
//! A `const`-evaluable scalar encoder looks attractive: **LLVM does not
//! constant-fold `_mm_cvtps_ph`**, so on an F16C build `Corners::all(8.0)`
//! emits `vbroadcastss` + `vcvtps2ph` and converts a literal at runtime on
//! every call, where plain arithmetic folds to one `movabsq` of the packed
//! bits. That much is real — confirmed in the assembly, and it needs
//! `#[inline(always)]`: at plain `#[inline]` LLVM declines the branchy
//! body and folds nothing.
//!
//! It was measured and rejected anyway, on two counts:
//!
//! - **The upside is nil.** Modelled as one conversion per opaque call,
//!   the folded form beat the intrinsic by 1.4 ns across 256 call sites —
//!   0.005 ns each, about two instructions. Constant sites also barely
//!   register in the `frame` profile's caller breakdown; the conversions
//!   there are the composer's per-quad geometry, never literals.
//! - **The downside is real.** No constructor is exclusively literal.
//!   `Spacing::all` takes `stroke_inset + aa_inset` in the composer's
//!   per-quad path, `Spacing::new` takes runtime extents in `layout::axis`,
//!   and `Corners::all` takes a computed radius in Slider, ProgressBar,
//!   Toggle and the scrollbars. Routing those to a scalar encoder trades
//!   one instruction for the ~60 the `runtime_scalar` arms below measure.
//!
//! Measuring it needs care, and two shapes of this bench were wrong before
//! this one: a loop over a constant input has its conversion hoisted by
//! LICM whether or not it folds, and a `black_box` per iteration costs
//! more than the conversion it guards. Both made the arms read identical.

use crate::bench::Run;
use crate::primitives::half_simd::{F16x4, f16x4_from_f32x4, f16x4_scaled, f16x4_to_f32x4};
use criterion::Criterion;
use half::f16;
use std::hint::black_box;

/// Conversions per iteration. One is a fraction of a nanosecond — under
/// criterion's timer resolution — so each arm runs a batch and the
/// reported figure is per batch. Large enough to amortise the loop, small
/// enough that the inputs stay L1-resident and this measures conversion
/// rather than memory.
const BATCH: usize = 256;

/// Lane quads spanning what the crate actually converts: corner radii and
/// spacing in the single digits, colour components in 0..1, physical
/// rects in the hundreds, and negatives (shadow offsets, inset spread).
/// Deliberately not uniform — f16 encode cost is exponent-dependent, and
/// a bench of `[1.0; 4]` would flatter the scalar path's branchy
/// subnormal and overflow handling.
fn inputs() -> Vec<[f32; 4]> {
    (0..BATCH)
        .map(|i| {
            let k = i as f32;
            [
                k * 0.37,
                k.mul_add(-1.5, 8.0),
                (k * 0.013).min(1.0),
                k.mul_add(3.25, 0.5),
            ]
        })
        .collect()
}

fn packed() -> Vec<[u16; 4]> {
    inputs().into_iter().map(f16x4_from_f32x4).collect()
}

/// `half`'s **scalar** conversion, lane by lane — the reference the SIMD
/// path is worth keeping over. Not a crate code path; it lives here so
/// the margin is measured rather than asserted.
///
/// `from_f32_const`, not `from_f32`: the latter runs `half`'s *own*
/// runtime F16C detection and calls the same intrinsic, so an arm built
/// on it compares the SIMD path against itself and reads as a ~20%
/// difference instead of the ~20x one below.
#[inline]
fn scalar_from_f32x4(src: [f32; 4]) -> [u16; 4] {
    [
        f16::from_f32_const(src[0]).to_bits(),
        f16::from_f32_const(src[1]).to_bits(),
        f16::from_f32_const(src[2]).to_bits(),
        f16::from_f32_const(src[3]).to_bits(),
    ]
}

#[inline]
fn scalar_to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    [
        f16::from_bits(bits[0]).to_f32_const(),
        f16::from_bits(bits[1]).to_f32_const(),
        f16::from_bits(bits[2]).to_f32_const(),
        f16::from_bits(bits[3]).to_f32_const(),
    ]
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let src = inputs();
    let bits = packed();

    let mut g = run.group(c);

    // The composer's shape: a value unknowable at compile time. Inputs are
    // laundered so neither side can see them.
    g.bench_function("from_f32x4/runtime_simd", |b| {
        b.iter(|| {
            let mut acc = [0u16; 4];
            for v in black_box(&src) {
                acc = f16x4_from_f32x4(black_box(*v));
            }
            acc
        })
    });
    g.bench_function("from_f32x4/runtime_scalar", |b| {
        b.iter(|| {
            let mut acc = [0u16; 4];
            for v in black_box(&src) {
                acc = scalar_from_f32x4(black_box(*v));
            }
            acc
        })
    });

    g.bench_function("to_f32x4/runtime_simd", |b| {
        b.iter(|| {
            let mut acc = [0.0f32; 4];
            for v in black_box(&bits) {
                acc = f16x4_to_f32x4(black_box(*v));
            }
            acc
        })
    });
    g.bench_function("to_f32x4/runtime_scalar", |b| {
        b.iter(|| {
            let mut acc = [0.0f32; 4];
            for v in black_box(&bits) {
                acc = scalar_to_f32x4(black_box(*v));
            }
            acc
        })
    });

    // The fused decode-multiply-encode the composer runs per quad, against
    // the two-step spelling beside it — `F16x4::scaled`'s doc quotes a
    // ratio, and this is what keeps that number honest as codegen moves.
    g.bench_function("scaled/fused", |b| {
        b.iter(|| {
            let mut acc = [0u16; 4];
            for v in black_box(&bits) {
                acc = f16x4_scaled(black_box(*v), black_box(1.75));
            }
            acc
        })
    });
    g.bench_function("scaled/composed", |b| {
        b.iter(|| {
            let mut acc = F16x4::ZERO;
            for v in black_box(&bits) {
                let k = black_box(1.75);
                acc = F16x4::from_lanes(F16x4::from_bits(black_box(*v)).lanes().map(|x| x * k));
            }
            acc
        })
    });

    g.finish();
}
