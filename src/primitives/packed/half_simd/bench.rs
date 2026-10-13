//! f16 pack/unpack: the crate's SIMD path against `half`'s scalar one, plus
//! the fused [`F16x4::scaled`]; each is a standing claim in [`super`]'s docs.
//!
//! ## Why there is no `const` arm
//!
//! LLVM does not constant-fold `_mm_cvtps_ph`, so `Corners::all(8.0)` converts
//! a literal at runtime on an F16C build. A `const` scalar encoder was measured
//! and rejected: the folded form gains 0.005 ns per site, and no constructor is
//! only literal (`Spacing::all`, `Corners::all` take computed values), where a
//! scalar encoder costs ~60 instructions (the `runtime_scalar` arms).
//!
//! A loop over a constant input has its conversion hoisted by LICM, and a
//! per-iteration `black_box` costs more than the conversion; both made the arms
//! read identical.

use crate::bench::fixture::{Backend, Kind, Scale};
use crate::primitives::packed::half_simd::F16x4;
use criterion::{Criterion, Throughput};
use half::f16;
use std::hint::black_box;

/// Conversions per iteration, batched as one is under criterion's timer resolution.
const BATCH: usize = 256;

/// Lane quads spanning what the crate converts, not uniform as encode cost depends on the exponent.
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

fn packed() -> Vec<F16x4> {
    inputs().into_iter().map(F16x4::from_lanes).collect()
}

/// `half`'s scalar conversion, the reference for the SIMD path. Uses
/// `from_f32_const`, as `from_f32` runs `half`'s own F16C detection.
#[inline]
const fn scalar_from_f32x4(src: [f32; 4]) -> [u16; 4] {
    [
        f16::from_f32_const(src[0]).to_bits(),
        f16::from_f32_const(src[1]).to_bits(),
        f16::from_f32_const(src[2]).to_bits(),
        f16::from_f32_const(src[3]).to_bits(),
    ]
}

#[inline]
const fn scalar_to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    [
        f16::from_bits(bits[0]).to_f32_const(),
        f16::from_bits(bits[1]).to_f32_const(),
        f16::from_bits(bits[2]).to_f32_const(),
        f16::from_bits(bits[3]).to_f32_const(),
    ]
}

/// `atomic/half_simd/cpu/<conversion>/<path>`: [`BATCH`] lane quads through the crate's SIMD
/// path and `half`'s scalar one. The inputs are a few kilobytes of arithmetic, so they are built
/// at once. Per quad.
pub(crate) fn atomic(c: &mut Criterion) {
    let src = inputs();
    let bits = packed();

    let mut g = Kind::Atomic.group(c, "half_simd", Scale::Sample);
    g.throughput(Throughput::Elements(BATCH as u64));

    g.bench_function(Backend::Cpu.case("from_f32x4/runtime_simd"), |b| {
        b.iter(|| {
            let mut acc = F16x4::ZERO;
            for v in black_box(&src) {
                acc = F16x4::from_lanes(black_box(*v));
            }
            acc
        });
    });
    g.bench_function(Backend::Cpu.case("from_f32x4/runtime_scalar"), |b| {
        b.iter(|| {
            let mut acc = [0u16; 4];
            for v in black_box(&src) {
                acc = scalar_from_f32x4(black_box(*v));
            }
            acc
        });
    });

    g.bench_function(Backend::Cpu.case("to_f32x4/runtime_simd"), |b| {
        b.iter(|| {
            let mut acc = [0.0f32; 4];
            for v in black_box(&bits) {
                acc = black_box(*v).lanes();
            }
            acc
        });
    });
    g.bench_function(Backend::Cpu.case("to_f32x4/runtime_scalar"), |b| {
        b.iter(|| {
            let mut acc = [0.0f32; 4];
            for v in black_box(&bits) {
                acc = scalar_to_f32x4(black_box(v.0));
            }
            acc
        });
    });

    // The fused decode-multiply-encode against the two-step form; keeps `F16x4::scaled`'s quoted ratio honest.
    g.bench_function(Backend::Cpu.case("scaled/fused"), |b| {
        b.iter(|| {
            let mut acc = F16x4::ZERO;
            for v in black_box(&bits) {
                acc = black_box(*v).scaled(black_box(1.75));
            }
            acc
        });
    });
    g.bench_function(Backend::Cpu.case("scaled/composed"), |b| {
        b.iter(|| {
            let mut acc = F16x4::ZERO;
            for v in black_box(&bits) {
                let k = black_box(1.75);
                acc = F16x4::from_lanes(black_box(*v).lanes().map(|x| x * k));
            }
            acc
        });
    });

    g.finish();
}
