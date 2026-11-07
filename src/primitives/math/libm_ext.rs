//! [`Libm`]: the transcendental float functions, with the same bits on every system.

/// The float functions whose precision Rust leaves to the platform: std's methods call the system's
/// libm, whose last bit differs between glibc, Apple's libm and the MSVC runtime, and between
/// versions of one ([`f32::powf`]), so a result would differ by system. These call one pure-Rust
/// libm on every system, the rust-lang project's port of musl's. `clippy.toml` refuses std's methods,
/// as `bevy_math` refuses them for its `ops` module
/// (<https://github.com/bevyengine/bevy/blob/main/crates/bevy_math/clippy.toml>). Refused: a function
/// of our own for each, which each needs its own accuracy tests. Cost: `libm` can be slower than a
/// system's. Each method is named for what it replaces, as an inherent method would hide a trait's
/// of one name.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the crate's tests call some the library does not")
)]
pub(crate) trait Libm: Copy {
    #[must_use]
    fn libm_sin(self) -> Self;
    #[must_use]
    fn libm_cos(self) -> Self;
    #[must_use]
    fn libm_tan(self) -> Self;
    /// The sine and the cosine, as [`f32::sin_cos`].
    fn libm_sin_cos(self) -> (Self, Self);
    #[must_use]
    fn libm_asin(self) -> Self;
    #[must_use]
    fn libm_acos(self) -> Self;
    #[must_use]
    fn libm_atan(self) -> Self;
    /// The angle of (`other`, `self`), as [`f32::atan2`].
    #[must_use]
    fn libm_atan2(self, other: Self) -> Self;
    #[must_use]
    fn libm_sinh(self) -> Self;
    #[must_use]
    fn libm_cosh(self) -> Self;
    #[must_use]
    fn libm_tanh(self) -> Self;
    #[must_use]
    fn libm_asinh(self) -> Self;
    #[must_use]
    fn libm_acosh(self) -> Self;
    #[must_use]
    fn libm_atanh(self) -> Self;
    #[must_use]
    fn libm_exp(self) -> Self;
    #[must_use]
    fn libm_exp2(self) -> Self;
    #[must_use]
    fn libm_exp_m1(self) -> Self;
    #[must_use]
    fn libm_ln(self) -> Self;
    #[must_use]
    fn libm_ln_1p(self) -> Self;
    #[must_use]
    fn libm_log2(self) -> Self;
    #[must_use]
    fn libm_log10(self) -> Self;
    /// The logarithm in `base`, as [`f32::log`]: `ln(self) / ln(base)`.
    #[must_use]
    fn libm_log(self, base: Self) -> Self;
    #[must_use]
    fn libm_powf(self, exponent: Self) -> Self;
    /// `self` to an integer power, by squaring in one fixed order, as `libm` has no such function
    /// and LLVM's `powi` leaves the order to the target. Bits a multiply and a divide settle.
    #[must_use]
    fn libm_powi(self, exponent: i32) -> Self;
    #[must_use]
    fn libm_cbrt(self) -> Self;
    #[must_use]
    fn libm_hypot(self, other: Self) -> Self;
}

use glam::Vec2;

/// `Vec2`'s constructor that std's float functions serve.
pub(crate) trait LibmVec2 {
    /// The unit vector at `angle` radians, in place of `Vec2::from_angle`, which `clippy.toml`
    /// refuses. Refused: `glam`'s `libm` feature, which also sends `floor`, `round` and `sqrt` of a
    /// scalar `Vec2` through `libm`, on hot paths, where their bits need no help.
    fn libm_from_angle(angle: f32) -> Self;
}

impl LibmVec2 for Vec2 {
    fn libm_from_angle(angle: f32) -> Self {
        let (sin, cos) = angle.libm_sin_cos();
        Self::new(cos, sin)
    }
}

macro_rules! impl_libm {
    ($float:ty, $sin:ident, $cos:ident, $tan:ident, $sincos:ident, $asin:ident, $acos:ident,
     $atan:ident, $atan2:ident, $sinh:ident, $cosh:ident, $tanh:ident, $asinh:ident, $acosh:ident,
     $atanh:ident, $exp:ident, $exp2:ident, $expm1:ident, $log:ident, $log1p:ident, $log2:ident,
     $log10:ident, $pow:ident, $cbrt:ident, $hypot:ident) => {
        impl Libm for $float {
            #[inline]
            fn libm_sin(self) -> Self {
                libm::$sin(self)
            }
            #[inline]
            fn libm_cos(self) -> Self {
                libm::$cos(self)
            }
            #[inline]
            fn libm_tan(self) -> Self {
                libm::$tan(self)
            }
            #[inline]
            fn libm_sin_cos(self) -> (Self, Self) {
                libm::$sincos(self)
            }
            #[inline]
            fn libm_asin(self) -> Self {
                libm::$asin(self)
            }
            #[inline]
            fn libm_acos(self) -> Self {
                libm::$acos(self)
            }
            #[inline]
            fn libm_atan(self) -> Self {
                libm::$atan(self)
            }
            #[inline]
            fn libm_atan2(self, other: Self) -> Self {
                libm::$atan2(self, other)
            }
            #[inline]
            fn libm_sinh(self) -> Self {
                libm::$sinh(self)
            }
            #[inline]
            fn libm_cosh(self) -> Self {
                libm::$cosh(self)
            }
            #[inline]
            fn libm_tanh(self) -> Self {
                libm::$tanh(self)
            }
            #[inline]
            fn libm_asinh(self) -> Self {
                libm::$asinh(self)
            }
            #[inline]
            fn libm_acosh(self) -> Self {
                libm::$acosh(self)
            }
            #[inline]
            fn libm_atanh(self) -> Self {
                libm::$atanh(self)
            }
            #[inline]
            fn libm_exp(self) -> Self {
                libm::$exp(self)
            }
            #[inline]
            fn libm_exp2(self) -> Self {
                libm::$exp2(self)
            }
            #[inline]
            fn libm_exp_m1(self) -> Self {
                libm::$expm1(self)
            }
            #[inline]
            fn libm_ln(self) -> Self {
                libm::$log(self)
            }
            #[inline]
            fn libm_ln_1p(self) -> Self {
                libm::$log1p(self)
            }
            #[inline]
            fn libm_log2(self) -> Self {
                libm::$log2(self)
            }
            #[inline]
            fn libm_log10(self) -> Self {
                libm::$log10(self)
            }
            #[inline]
            fn libm_log(self, base: Self) -> Self {
                libm::$log(self) / libm::$log(base)
            }
            #[inline]
            fn libm_powf(self, exponent: Self) -> Self {
                libm::$pow(self, exponent)
            }
            #[inline]
            fn libm_powi(self, exponent: i32) -> Self {
                let mut power = 1.0;
                let mut square = self;
                let mut left = exponent.unsigned_abs();
                while left != 0 {
                    if left & 1 == 1 {
                        power *= square;
                    }
                    square *= square;
                    left >>= 1;
                }
                if exponent < 0 { 1.0 / power } else { power }
            }
            #[inline]
            fn libm_cbrt(self) -> Self {
                libm::$cbrt(self)
            }
            #[inline]
            fn libm_hypot(self, other: Self) -> Self {
                libm::$hypot(self, other)
            }
        }
    };
}

impl_libm!(
    f32, sinf, cosf, tanf, sincosf, asinf, acosf, atanf, atan2f, sinhf, coshf, tanhf, asinhf,
    acoshf, atanhf, expf, exp2f, expm1f, logf, log1pf, log2f, log10f, powf, cbrtf, hypotf
);
impl_libm!(
    f64, sin, cos, tan, sincos, asin, acos, atan, atan2, sinh, cosh, tanh, asinh, acosh, atanh,
    exp, exp2, expm1, log, log1p, log2, log10, pow, cbrt, hypot
);

#[cfg(test)]
mod tests {
    use crate::primitives::math::libm_ext::{Libm, LibmVec2};
    use glam::Vec2;

    /// Squares, by hand: each power is the product of the squares its exponent's bits name, and a
    /// negative one its inverse, and a power of nothing is 1, as `powi` gives even for a NaN.
    #[test]
    fn an_integer_power_multiplies_in_a_fixed_order() {
        assert_eq!(3.0_f32.libm_powi(4), 81.0);
        assert_eq!(3.0_f64.libm_powi(5), 243.0);
        assert_eq!(2.0_f32.libm_powi(-2), 0.25);
        assert_eq!(2.0_f64.libm_powi(31), 2_147_483_648.0);
        assert_eq!(5.0_f32.libm_powi(1), 5.0);
        assert_eq!(0.0_f32.libm_powi(0), 1.0);
        assert_eq!(f32::NAN.libm_powi(0), 1.0);
        assert_eq!(0.0_f64.libm_powi(-1), f64::INFINITY);
        // The magnitude of `i32::MIN` has no `i32`; a base of 1 shows the loop ends on it.
        assert_eq!(1.0_f64.libm_powi(i32::MIN), 1.0);
    }

    /// One value of each function, whose result is exact or whose rounding is settled, in each
    /// width: the two names of each width's table reach the function they say.
    #[test]
    fn each_method_reaches_its_function() {
        assert_eq!(0.0_f32.libm_sin(), 0.0);
        assert_eq!(0.0_f64.libm_cos(), 1.0);
        assert_eq!(0.0_f32.libm_tan(), 0.0);
        assert_eq!(0.0_f64.libm_sin_cos(), (0.0, 1.0));
        assert_eq!(0.0_f32.libm_asin(), 0.0);
        assert_eq!(1.0_f64.libm_acos(), 0.0);
        assert_eq!(0.0_f32.libm_atan(), 0.0);
        assert_eq!(0.0_f64.libm_atan2(1.0), 0.0);
        assert_eq!(0.0_f32.libm_sinh(), 0.0);
        assert_eq!(0.0_f64.libm_cosh(), 1.0);
        assert_eq!(0.0_f32.libm_tanh(), 0.0);
        assert_eq!(0.0_f64.libm_asinh(), 0.0);
        assert_eq!(1.0_f32.libm_acosh(), 0.0);
        assert_eq!(0.0_f64.libm_atanh(), 0.0);
        assert_eq!(0.0_f32.libm_exp(), 1.0);
        assert_eq!(3.0_f64.libm_exp2(), 8.0);
        assert_eq!(0.0_f32.libm_exp_m1(), 0.0);
        assert_eq!(1.0_f64.libm_ln(), 0.0);
        assert_eq!(0.0_f32.libm_ln_1p(), 0.0);
        assert_eq!(8.0_f64.libm_log2(), 3.0);
        assert_eq!(1000.0_f32.libm_log10(), 3.0);
        assert_eq!(1.0_f64.libm_log(2.0), 0.0);
        assert_eq!(2.0_f32.libm_powf(3.0), 8.0);
        assert_eq!(27.0_f64.libm_cbrt(), 3.0);
        assert_eq!(3.0_f32.libm_hypot(4.0), 5.0);
    }

    /// The unit vector at a quarter turn, and one between, whose parts are its sine and cosine.
    #[test]
    fn a_unit_vector_points_at_its_angle() {
        assert_eq!(Vec2::libm_from_angle(0.0), Vec2::X);
        let quarter = Vec2::libm_from_angle(std::f32::consts::FRAC_PI_2);
        assert_eq!(quarter.y, 1.0);
        assert!(quarter.x.abs() < 1e-7);
        let (sin, cos) = 0.5_f32.libm_sin_cos();
        assert_eq!(Vec2::libm_from_angle(0.5), Vec2::new(cos, sin));
        assert_ne!(Vec2::libm_from_angle(0.5), Vec2::libm_from_angle(0.6));
    }
}
