//! `half`'s slice path, for every architecture without its own file: `fcvtl` on aarch64-fp16, scalar elsewhere.

use half::slice::HalfFloatSliceExt as _;

#[inline]
pub(super) fn to_f32x4(bits: [u16; 4]) -> [f32; 4] {
    let lanes: &[half::f16; 4] = bytemuck::cast_ref(&bits);
    let mut out = [0.0f32; 4];
    lanes.convert_to_f32_slice(&mut out);
    out
}

#[inline]
pub(super) fn from_f32x4(src: [f32; 4]) -> [u16; 4] {
    let mut out = [half::f16::ZERO; 4];
    out.convert_from_f32_slice(&src);
    bytemuck::cast(out)
}

#[inline]
pub(super) fn scaled(bits: [u16; 4], k: f32, limit: f32) -> [u16; 4] {
    from_f32x4(to_f32x4(bits).map(|v| (v * k).clamp(-limit, limit)))
}
