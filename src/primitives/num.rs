use crate::primitives::approx::approx_zero;

/// Marker trait for primitive numeric types accepted by `From` impls on
/// `Sizing`, `Size`, `Corners`, `Spacing`, etc.
pub(crate) trait Num: Copy {
    fn as_f32(self) -> f32;
}

macro_rules! impl_num {
    ($($t:ty),*) => {
        $(
            impl Num for $t {
                fn as_f32(self) -> f32 { self as f32 }
            }
        )*
    };
}

impl_num!(f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// Canonicalize an `f32` so equal-up-to-`EPS` values hash identically:
/// collapse any value with `|f| <= EPS` (including `-0.0`, `+0.0`, and
/// sub-`EPS` subnormals) to a single zero bit pattern, and every NaN
/// to a single quiet-NaN. Shared by every content-hash that includes
/// f32 fields (gradient stops, axes, atlas row keys) so they can't
/// drift apart — and so visually identical inputs (e.g. an angle of
/// `1e-8` vs `0.0`) share a cache row.
#[inline]
pub(crate) fn canon_bits(f: f32) -> u32 {
    if f.is_nan() {
        f32::NAN.to_bits()
    } else if approx_zero(f) {
        0u32
    } else {
        f.to_bits()
    }
}
