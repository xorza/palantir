//! The value vocabulary every other layer is written in (geometry, layout, paint, text, identity, math, packed
//! encodings) plus the lane macro for four-f16 types. Below every layer but `common`, its one dependency: nothing here reaches up into animation, scene, layout or renderer.

/// The surface a four-lane [`F16x4`](crate::primitives::packed::half_simd::F16x4) newtype shares with its
/// siblings: lane access, `Debug` naming the lanes, `Animatable` lane by lane, scalar `From`, serde
/// forwarders. For [`Corners`](crate::Corners)
/// and [`Spacing`](crate::Spacing); `RgbaF16` and `FillAxis` have no lane names or wire format and derive `Debug`.
/// `floor` gives the lanes a least value, which every blend and motion is coerced to, or `None`;
/// `two_node`, `rule` and `valid` are the type's [`LaneCodec`](crate::primitives::packed::serde::LaneCodec)
/// policy, whose lane names are the lanes', and from which the checked pack `checked` follows,
/// so one predicate refuses a lane at a call site and in a file alike.
macro_rules! f16x4_lanes {
    (
        $t:ident,
        [$($lane:ident),+ $(,)?],
        floor = $floor:expr,
        two_node = $two_node:expr,
        rule = $rule:expr,
        valid = $valid:path $(,)?
    ) => {
        impl $crate::primitives::packed::serde::LaneCodec for $t {
            const FIELDS: &'static [&'static str] = &[$(stringify!($lane)),+];
            const TWO_NODE: $crate::primitives::packed::serde::TwoNode = $two_node;
            const LANE_RULE: &'static str = $rule;

            fn lane_is_valid(lane: f32) -> bool {
                $valid(lane)
            }

            fn from_valid_lanes(lanes: [f32; 4]) -> Self {
                debug_assert!(lanes.into_iter().all($valid), "{}", Self::LANE_RULE);
                Self($crate::primitives::packed::half_simd::F16x4::from_lanes(lanes))
            }

            fn to_lane_array(&self) -> [f32; 4] {
                self.to_array()
            }
        }

        impl $t {
            /// Four lanes in lane order, each one [`LaneCodec::lane_is_valid`] holds.
            ///
            /// [`LaneCodec::lane_is_valid`]: crate::primitives::packed::serde::LaneCodec::lane_is_valid
            ///
            /// # Panics
            ///
            /// Panics on a lane the type cannot hold, at the caller's site.
            #[inline]
            #[track_caller]
            fn checked(lanes: [f32; 4]) -> Self {
                use $crate::primitives::packed::serde::LaneCodec as _;
                assert!(lanes.into_iter().all($valid), "{}", Self::LANE_RULE);
                Self::from_valid_lanes(lanes)
            }
        }

        f16x4_lanes!(@impl $t, [$($lane),+], $floor);
    };
    (@impl $t:ident, [$($lane:ident),+], $floor:expr) => {
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
        /// rounds it to a relative 2^-11, under 0.01 px a frame below 16 px. Coerced to the lanes'
        /// floor, as a curve that overshoots can leave a blend under it.
        impl $crate::primitives::math::lerp::Lerp for $t {
            #[inline]
            fn lerp(a: &Self, b: &Self, t: f32) -> Self {
                if t == 0.0 {
                    return *a;
                }
                if t == 1.0 {
                    return *b;
                }
                let mut blend = Self::lanewise(*a, *b, |a, b| {
                    <f32 as $crate::primitives::math::lerp::Lerp>::lerp(&a, &b, t)
                });
                blend.coerce();
                blend
            }
        }

        /// Per lane, packed back saturating as [`Lerp`](crate::widget::Lerp) is.
        impl $crate::primitives::math::animatable::Animatable for $t {
            #[inline]
            fn sub(&self, other: &Self) -> Self {
                Self::lanewise(*self, *other, |a, b| a - b)
            }
            #[inline]
            fn add(&self, other: &Self) -> Self {
                Self::lanewise(*self, *other, |a, b| a + b)
            }
            #[inline]
            fn scale(&self, k: f32) -> Self {
                Self($crate::primitives::packed::half_simd::F16x4::from_lanes_saturating(
                    self.to_array().map(|lane| lane * k),
                ))
            }
            #[inline]
            fn magnitude_squared(&self) -> f32 {
                self.to_array().iter().map(|lane| lane * lane).sum()
            }
            /// Each lane a length in logical pixels.
            #[inline]
            fn settle_distance_squared(&self) -> f32 {
                let eps = $crate::primitives::math::animatable::PX_SETTLE_EPS;
                self.magnitude_squared() / (eps * eps)
            }
            #[inline]
            fn zero() -> Self {
                Self::ZERO
            }
            #[inline]
            fn from_motion(mut motion: Self) -> Self {
                motion.coerce();
                motion
            }
        }

        impl $t {
            /// Raise each lane to the lanes' floor, as a curve overshooting it leaves it, as CSS
            /// clamps an interpolated value into its property's range (CSS Values 4, "Range
            /// Checking"); nothing with no floor.
            #[inline]
            fn coerce(&mut self) {
                let floor: Option<f32> = $floor;
                if let Some(floor) = floor {
                    *self = Self(
                        $crate::primitives::packed::half_simd::F16x4::from_lanes_saturating(
                            self.to_array()
                                .map(|lane| $crate::primitives::math::domain::raised_to(lane, floor)),
                        ),
                    );
                }
            }

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

pub(crate) mod effect;
pub(crate) mod geometry;
pub(crate) mod identity;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod math;
pub(crate) mod packed;
pub(crate) mod paint;
pub(crate) mod preference;
pub(crate) mod reduction;
pub(crate) mod text;
pub(crate) mod wgsl;
