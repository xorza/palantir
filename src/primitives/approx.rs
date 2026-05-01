/// Float comparisons at UI tolerance.
///
/// `EPS = 1e-4` is below 8-bit color precision (1/255 ≈ 4e-3) and sub-pixel
/// position resolution at typical display scales, so differences smaller
/// than this are invisible to the user.
///
/// `approx_eq` uses combined absolute + relative tolerance — absolute near
/// zero, relative scaled by magnitude for larger values — which matches the
/// `approx` crate's `relative_eq!` default and the practice from Bruce
/// Dawson's "Comparing Floating Point Numbers". Pure absolute tolerance
/// breaks at large magnitudes; pure relative tolerance breaks near zero.
pub trait ApproxF32: Copy {
    const EPS: f32 = 1.0e-4;

    fn approx_zero(self) -> bool;
    fn approx_eq(self, other: Self) -> bool;
}

impl ApproxF32 for f32 {
    fn approx_zero(self) -> bool {
        self.abs() < Self::EPS
    }
    fn approx_eq(self, other: f32) -> bool {
        let diff = (self - other).abs();
        if diff <= Self::EPS {
            return true;
        }
        let largest = self.abs().max(other.abs());
        diff <= Self::EPS * largest
    }
}

impl ApproxF32 for glam::Vec2 {
    fn approx_zero(self) -> bool {
        self.x.approx_zero() && self.y.approx_zero()
    }
    fn approx_eq(self, other: Self) -> bool {
        self.x.approx_eq(other.x) && self.y.approx_eq(other.y)
    }
}

impl ApproxF32 for super::Size {
    fn approx_zero(self) -> bool {
        self.w.approx_zero() && self.h.approx_zero()
    }
    fn approx_eq(self, other: Self) -> bool {
        self.w.approx_eq(other.w) && self.h.approx_eq(other.h)
    }
}
