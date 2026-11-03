//! The value vocabulary every other layer is written in (geometry, layout, paint, text, identity, math, packed
//! encodings) plus the lane macro for four-f16 types. Below every layer but `common`, its one dependency: nothing here reaches up into animation, scene, layout or renderer.

/// The surface a four-lane [`F16x4`](crate::primitives::packed::half_simd::F16x4) newtype shares with its
/// siblings: lane access, `Debug` naming the lanes, `Animatable` lane by lane, scalar `From`, serde
/// forwarders. For [`Corners`](crate::Corners)
/// and [`Spacing`](crate::Spacing); `RgbaF16` and `FillAxis` have no lane names or wire format and derive `Debug`.
macro_rules! f16x4_lanes {
    ($t:ident, [$($lane:ident),+ $(,)?]) => {
        impl $t {
            /// All lanes zero.
            pub const ZERO: Self = Self($crate::primitives::packed::half_simd::F16x4::ZERO);

            /// All four lanes unpacked at once (a single `vcvtph2ps` on x86-f16c), for hot sites reading 3+ lanes.
            #[inline]
            pub fn to_array(self) -> [f32; 4] {
                self.0.lanes()
            }

            /// True if any lane is NaN. `const`, and the [`NanCheck`] impl delegates here; a NaN lane poisons every extent derived from it.
            ///
            /// [`NanCheck`]: crate::primitives::math::nan::NanCheck
            #[inline]
            pub(crate) const fn has_nan(self) -> bool {
                self.0.has_nan()
            }
        }

        impl ::std::fmt::Debug for $t {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                let [$($lane),+] = self.to_array();
                f.debug_struct(stringify!($t))
                    $(.field(stringify!($lane), &$lane))+
                    .finish()
            }
        }

        impl $crate::primitives::math::nan::NanCheck for $t {
            #[inline]
            fn has_nan(&self) -> bool {
                $t::has_nan(*self)
            }
        }

        /// Per lane in f32, as `f32` blends, each result packed back saturating at ±65504: an f16
        /// rounds it to a relative 2^-11, under 0.01 px a frame below 16 px.
        impl $crate::primitives::math::lerp::Lerp for $t {
            #[inline]
            fn lerp(a: Self, b: Self, t: f32) -> Self {
                if t == 0.0 {
                    return a;
                }
                if t == 1.0 {
                    return b;
                }
                Self::lanewise(a, b, |a, b| {
                    <f32 as $crate::primitives::math::lerp::Lerp>::lerp(a, b, t)
                })
            }
        }

        /// Per lane, packed back saturating as [`Lerp`](crate::widget::Lerp) is.
        impl $crate::primitives::math::animatable::Animatable for $t {
            #[inline]
            fn sub(self, other: Self) -> Self {
                Self::lanewise(self, other, |a, b| a - b)
            }
            #[inline]
            fn add(self, other: Self) -> Self {
                Self::lanewise(self, other, |a, b| a + b)
            }
            #[inline]
            fn scale(self, k: f32) -> Self {
                Self($crate::primitives::packed::half_simd::F16x4::from_lanes_saturating(
                    self.to_array().map(|lane| lane * k),
                ))
            }
            #[inline]
            fn magnitude_squared(self) -> f32 {
                self.to_array().iter().map(|lane| lane * lane).sum()
            }
            #[inline]
            fn zero() -> Self {
                Self::ZERO
            }
        }

        impl $t {
            /// `f` over each pair of lanes, packed back saturating.
            #[inline]
            fn lanewise(a: Self, b: Self, f: impl Fn(f32, f32) -> f32) -> Self {
                let (a, b) = (a.to_array(), b.to_array());
                Self($crate::primitives::packed::half_simd::F16x4::from_lanes_saturating(
                    ::std::array::from_fn(|i| f(a[i], b[i])),
                ))
            }
        }

        impl<T: $crate::primitives::math::num::Number> From<T> for $t {
            #[track_caller]
            fn from(v: T) -> Self {
                Self::all(v.to_f32())
            }
        }

        impl ::serde::Serialize for $t {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $crate::primitives::packed::serde::serialize_lanes(self, serializer)
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $t {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                $crate::primitives::packed::serde::deserialize_lanes(deserializer)
            }
        }
    };
}

pub(crate) mod geometry;
pub(crate) mod identity;
pub(crate) mod layout;
pub(crate) mod math;
pub(crate) mod packed;
pub(crate) mod paint;
pub(crate) mod text;
