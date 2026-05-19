//! Pack/unpack throughput for the 4-lane f16↔f32 conversion used by
//! `Spacing`/`Corners`/`Color`/`Brush` storage. Compares the three
//! candidate paths so we can see what Apple Silicon leaves on the table:
//!
//!   1. `palantir` — `crate::primitives::half_simd::{f16x4_to_f32x4,f16x4_from_f32x4}`.
//!      Today: on x86_64+f16c calls `_mm_cvtph_ps`/`_mm_cvtps_ph`
//!      directly; on aarch64+fp16 routes through `half`'s slice path.
//!   2. `half_slice` — bare `half::slice::HalfFloatSliceExt` on a
//!      4-element slice. Same code path the non-x86 palantir wrapper
//!      uses, measured without our wrapper so we can see how much (if
//!      any) the chunked-slice plumbing costs.
//!   3. `aarch64_asm` — direct `fcvtl`/`fcvtn` inline asm on
//!      aarch64+fp16, mirroring `half::binary16::arch::aarch64` but
//!      called from a leaf `#[inline(always)]` fn with no slice
//!      indirection.  This is the "what could palantir achieve if we
//!      added a native aarch64 path" candidate.
//!
//! Each case measures a 1024-iteration tight loop over a fixed
//! `[u16; 4]` / `[f32; 4]` input, with `black_box` on the result.
//! 1024 iterations keeps the workload above criterion's measurement
//! floor while staying small enough that the data stays in L1.
//!
//! Run:
//! ```sh
//! cargo bench --bench half_simd
//! cargo bench --bench half_simd -- 'unpack/'   # one direction
//! ```

use criterion::{Criterion, criterion_group, criterion_main};
use half::f16;
use half::slice::HalfFloatSliceExt;
use palantir::primitives::half_simd::test_support::{f16x4_from_f32x4, f16x4_to_f32x4};
use std::hint::black_box;

const ITERS: usize = 1024;

fn sample_bits() -> [u16; 4] {
    [
        f16::from_f32(1.1).to_bits(),
        f16::from_f32(-2.5).to_bits(),
        f16::from_f32(0.125).to_bits(),
        f16::from_f32(1234.5).to_bits(),
    ]
}

fn sample_floats() -> [f32; 4] {
    [1.1, -2.5, 0.125, 1234.5]
}

#[cfg(all(target_arch = "aarch64", target_feature = "fp16"))]
mod aarch64_direct {
    use core::arch::asm;
    use core::arch::aarch64::{float32x4_t, uint16x4_t};

    // `target_feature = "fp16"` is already enabled by default on
    // aarch64-apple-darwin (the only target this `cfg` block matches
    // in practice), so we don't need `#[target_feature(enable=...)]`
    // — which would forbid `#[inline(always)]` per rustc issue
    // #145574. The `asm!` block uses the H/S vreg aliases that the
    // base aarch64 ISA already exposes.
    #[inline(always)]
    pub unsafe fn unpack(bits: [u16; 4]) -> [f32; 4] {
        let vec: uint16x4_t = unsafe { core::mem::transmute(bits) };
        let result: float32x4_t;
        unsafe {
            asm!(
                "fcvtl {0:v}.4s, {1:v}.4h",
                out(vreg) result,
                in(vreg) vec,
                options(pure, nomem, nostack));
            core::mem::transmute(result)
        }
    }

    #[inline(always)]
    pub unsafe fn pack(src: [f32; 4]) -> [u16; 4] {
        let vec: float32x4_t = unsafe { core::mem::transmute(src) };
        let result: uint16x4_t;
        unsafe {
            asm!(
                "fcvtn {0:v}.4h, {1:v}.4s",
                out(vreg) result,
                in(vreg) vec,
                options(pure, nomem, nostack));
            core::mem::transmute(result)
        }
    }
}

fn bench_unpack(c: &mut Criterion) {
    let mut g = c.benchmark_group("unpack");
    let bits = sample_bits();

    g.bench_function("palantir", |b| {
        b.iter(|| {
            let mut acc = [0.0f32; 4];
            for _ in 0..ITERS {
                let out = f16x4_to_f32x4(black_box(bits));
                acc[0] += out[0];
                acc[1] += out[1];
                acc[2] += out[2];
                acc[3] += out[3];
            }
            black_box(acc)
        })
    });

    g.bench_function("half_slice", |b| {
        b.iter(|| {
            let mut acc = [0.0f32; 4];
            for _ in 0..ITERS {
                let bits_v = black_box(bits);
                let src: &[f16; 4] = bytemuck::cast_ref(&bits_v);
                let mut out = [0.0f32; 4];
                src.as_slice().convert_to_f32_slice(&mut out);
                acc[0] += out[0];
                acc[1] += out[1];
                acc[2] += out[2];
                acc[3] += out[3];
            }
            black_box(acc)
        })
    });

    #[cfg(all(target_arch = "aarch64", target_feature = "fp16"))]
    g.bench_function("aarch64_asm", |b| {
        b.iter(|| {
            let mut acc = [0.0f32; 4];
            for _ in 0..ITERS {
                // SAFETY: bench compiled with `target_feature = "fp16"`
                // statically enabled on aarch64-apple-darwin.
                let out = unsafe { aarch64_direct::unpack(black_box(bits)) };
                acc[0] += out[0];
                acc[1] += out[1];
                acc[2] += out[2];
                acc[3] += out[3];
            }
            black_box(acc)
        })
    });

    g.finish();
}

fn bench_pack(c: &mut Criterion) {
    let mut g = c.benchmark_group("pack");
    let src = sample_floats();

    g.bench_function("palantir", |b| {
        b.iter(|| {
            let mut acc = [0u32; 4];
            for _ in 0..ITERS {
                let out = f16x4_from_f32x4(black_box(src));
                acc[0] = acc[0].wrapping_add(out[0] as u32);
                acc[1] = acc[1].wrapping_add(out[1] as u32);
                acc[2] = acc[2].wrapping_add(out[2] as u32);
                acc[3] = acc[3].wrapping_add(out[3] as u32);
            }
            black_box(acc)
        })
    });

    g.bench_function("half_slice", |b| {
        b.iter(|| {
            let mut acc = [0u32; 4];
            for _ in 0..ITERS {
                let src_v = black_box(src);
                let mut out = [f16::ZERO; 4];
                out.as_mut_slice().convert_from_f32_slice(&src_v);
                acc[0] = acc[0].wrapping_add(out[0].to_bits() as u32);
                acc[1] = acc[1].wrapping_add(out[1].to_bits() as u32);
                acc[2] = acc[2].wrapping_add(out[2].to_bits() as u32);
                acc[3] = acc[3].wrapping_add(out[3].to_bits() as u32);
            }
            black_box(acc)
        })
    });

    #[cfg(all(target_arch = "aarch64", target_feature = "fp16"))]
    g.bench_function("aarch64_asm", |b| {
        b.iter(|| {
            let mut acc = [0u32; 4];
            for _ in 0..ITERS {
                // SAFETY: see unpack/aarch64_asm.
                let out = unsafe { aarch64_direct::pack(black_box(src)) };
                acc[0] = acc[0].wrapping_add(out[0] as u32);
                acc[1] = acc[1].wrapping_add(out[1] as u32);
                acc[2] = acc[2].wrapping_add(out[2] as u32);
                acc[3] = acc[3].wrapping_add(out[3] as u32);
            }
            black_box(acc)
        })
    });

    g.finish();
}

criterion_group!(benches, bench_unpack, bench_pack);
criterion_main!(benches);
