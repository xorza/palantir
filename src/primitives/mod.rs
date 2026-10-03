//! The value vocabulary every other layer is written in — geometry,
//! layout, paint, text and identity, the math they share, and their packed
//! encodings — plus the shared lane macro the four-f16 packed types are
//! built from.
//!
//! A leaf layer: nothing here reaches up into scene, layout or renderer,
//! which is what lets all three depend on one definition of a rect, a
//! colour or an id.

/// The surface a four-lane [`F16x4`](crate::primitives::packed::half_simd::F16x4)
/// newtype shares with its siblings: lane access, a `Debug` that names
/// the four lanes, scalar `From`, and the serde forwarders.
///
/// [`Corners`](crate::Corners) and [`Spacing`](crate::Spacing) are the
/// same eight bytes with different names for the lanes, and every item
/// here was written out twice with only those names differing. What is
/// *not* here is what actually differs: the constructors each offers,
/// and which two-value shorthand its wire format expands.
///
/// Not every `F16x4` newtype wants this. `RgbaF16` and `FillAxis` are
/// the same packing with a different surface — no lane names to print,
/// no wire format — and derive `Debug` like ordinary structs.
macro_rules! f16x4_lanes {
    ($t:ident, [$($lane:ident),+ $(,)?]) => {
        impl $t {
            /// The zero of every lane.
            pub const ZERO: Self = Self($crate::primitives::packed::half_simd::F16x4::ZERO);

            /// All four lanes unpacked at once — a single `vcvtph2ps`
            /// on x86-f16c, a scalar walk elsewhere. Use at hot sites
            /// that read 3+ lanes, to amortize the dispatch over all
            /// of them.
            #[inline]
            pub fn as_array(self) -> [f32; 4] {
                self.0.lanes()
            }

            /// Inverse of [`Self::as_array`] — the four-lane f32→f16
            /// pack. Use at hot sites that compute all four.
            #[inline]
            pub fn from_array(v: [f32; 4]) -> Self {
                Self($crate::primitives::packed::half_simd::F16x4::from_lanes(v))
            }

            /// True if any lane is NaN. `const`, so the predicates that
            /// gate on it can be too; the [`NanCheck`] impl below
            /// delegates here rather than keeping a second copy. A NaN
            /// lane poisons every extent derived from it, and it is
            /// cheaper to refuse one than to find it in a frame that came
            /// out blank.
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
