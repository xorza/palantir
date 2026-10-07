//! The value vocabulary every other layer is written in (geometry, layout, paint, text, identity, math, packed
//! encodings) plus the lane macro for four-f16 types. A leaf layer: nothing here reaches up into scene, layout or renderer.

/// The surface a four-lane [`F16x4`](crate::primitives::packed::half_simd::F16x4) newtype shares with its
/// siblings: lane access, `Debug` naming the lanes, scalar `From`, serde forwarders. For [`Corners`](crate::Corners)
/// and [`Spacing`](crate::Spacing); `RgbaF16` and `FillAxis` have no lane names or wire format and derive `Debug`.
macro_rules! f16x4_lanes {
    ($t:ident, [$($lane:ident),+ $(,)?]) => {
        impl $t {
            /// All lanes zero.
            pub const ZERO: Self = Self($crate::primitives::packed::half_simd::F16x4::ZERO);

            /// All four lanes unpacked at once (a single `vcvtph2ps` on x86-f16c), for hot sites reading 3+ lanes.
            #[inline]
            pub fn as_array(self) -> [f32; 4] {
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
                let [$($lane),+] = self.as_array();
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

        impl<T: $crate::primitives::math::num::Num> From<T> for $t {
            fn from(v: T) -> Self {
                Self::all(v.as_f32())
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
