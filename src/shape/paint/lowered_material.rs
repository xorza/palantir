//! A material lowered: the effect it paints through and what it captures, its parameters' words in
//! the record store and its luminance ceiling and floor; and what a material reads each record.

use std::hash::{self, Hasher as _};

use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::effect::effect_registry::ResolvedMaterial;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::shape::paint::capture_params::{CaptureParams, CaptureReach};
use crate::shape::record_store::{RecordStore, Resolve};

/// What a box's material reads this record, as the reductions decide it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MaterialRead {
    /// Its effect, the one the registry resolved its name to, over what is painted under the box,
    /// captured and blurred, read as far past the box as the effect declares from the
    /// material's parameters.
    Capture(ResolvedMaterial),
    /// Its fallback in place of the box's fill, the box a plain one with no capture, blur or
    /// effect; `theme` where it names the theme's surface.
    Fallback { theme: RgbaF32 },
}

impl MaterialRead {
    /// What `material` reads under `resolve`'s reductions, its effect resolved in its registry:
    /// its fallback in place of its fill while transparency is reduced, the theme's surface being
    /// `resolve`'s, else its effect; `None` where its effect's alternative for reduced motion or
    /// power is nothing, where the box's fill paints as it is.
    ///
    /// # Panics
    ///
    /// When no effect is loaded under the material's name, or one no material wears, or one of
    /// another lane count, whatever is reduced: a material that cannot paint is a logic error in
    /// every mode.
    #[track_caller]
    pub(crate) fn resolve(material: &Material, resolve: Resolve<'_>) -> Option<Self> {
        let Resolve {
            effects,
            reductions,
            material_fallback,
        } = resolve;
        let resolved = effects.resolve_material(material, reductions);
        if reductions.transparency.is_reduced() {
            return Some(Self::Fallback {
                theme: material_fallback,
            });
        }
        resolved.map(Self::Capture)
    }
}

/// A material a box wears and what it reads this record: the one a paint lowers with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WornMaterial<'a> {
    pub(crate) material: &'a Material,
    pub(crate) read: MaterialRead,
}

/// A capturing [`Material`] as a record keeps it, packed: its effect, a capture whose reach the
/// effect declared, the first of its parameters' words in the record store with their hash, and
/// the luminance bounds. `max_luminance` is the ceiling's f16 bits, rounded down so the material
/// never shows brighter than its author allowed, and 65504 for none; `min_luminance` the floor's,
/// rounded up so it never shows darker, and 0 for none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LoweredMaterial {
    effect: EffectId,
    pub(crate) capture: CaptureParams,
    /// The hash of the parameters' bytes, as their words are frame-local.
    input_hash: u64,
    /// The parameters' words in the record store, which the record pass that staged them keeps.
    pub(crate) params: Span,
    max_luminance: u16,
    min_luminance: u16,
}

impl LoweredMaterial {
    /// `material` through the effect `resolved` names, reading its reach past the box, its
    /// parameters staged in `store`. [`Material::validate`] holds an authored material within its
    /// kinds, and a theme transition's blend may carry it past them, as
    /// [`Easing::OutBack`](crate::Easing::OutBack) does past an end: here, where it starts to
    /// paint, its blur and floor coerce into their kinds and saturate at the lanes, as the lens of
    /// a reach does.
    pub(crate) fn lower(
        store: &mut RecordStore,
        material: &Material,
        resolved: ResolvedMaterial,
    ) -> Self {
        let ResolvedMaterial { id: effect, reach } = resolved;
        let bytes = material.effect.bytes();
        let mut h = Hasher::new();
        h.pod_slice(bytes);
        Self {
            effect,
            capture: CaptureParams {
                sigma: F16x4::saturate_lane(domain::raised_to(material.backdrop_blur, 0.0)),
                reach: CaptureReach::of(reach),
            },
            params: store.stage_effect_params(bytes),
            input_hash: h.finish(),
            max_luminance: material
                .max_luminance
                .map_or(half::f16::MAX.to_bits(), f16_at_or_below),
            min_luminance: material
                .min_luminance
                .map_or(0, |floor| f16_at_or_above(domain::raised_to(floor, 0.0))),
        }
    }

    /// The effect it paints through.
    pub(crate) const fn effect(self) -> EffectId {
        self.effect
    }

    /// The highest relative luminance the material shows: its ceiling, or 65504 with none.
    pub(crate) fn max_luminance(self) -> f32 {
        half::f16::from_bits(self.max_luminance).to_f32()
    }

    /// The lowest relative luminance the material shows: its floor, or 0 with none.
    pub(crate) fn min_luminance(self) -> f32 {
        half::f16::from_bits(self.min_luminance).to_f32()
    }
}

/// Everything but the frame-local `params`, whose bytes `input_hash` stands for.
impl hash::Hash for LoweredMaterial {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.effect.0);
        self.capture.hash(state);
        state.write_u64(self.input_hash);
        state.write_u16(self.max_luminance);
        state.write_u16(self.min_luminance);
    }
}

impl NanCheck for LoweredMaterial {
    fn has_nan(&self) -> bool {
        self.capture.has_nan()
            || half::f16::from_bits(self.max_luminance).is_nan()
            || half::f16::from_bits(self.min_luminance).is_nan()
    }
}

/// The bits of the f16 nearest `v` at or below it, `v` a factor saturated at 65504: a ceiling
/// that rounded up would let a material show brighter than its author derived.
fn f16_at_or_below(v: f32) -> u16 {
    debug_assert!(v >= 0.0, "a ceiling is a factor: {v}");
    let v = F16x4::saturate_lane(v);
    let nearest = half::f16::from_f32(v);
    if nearest.to_f32() > v {
        nearest.to_bits() - 1
    } else {
        nearest.to_bits()
    }
}

/// The bits of the f16 nearest `v` at or above it, `v` a factor saturated at 65504: a floor that
/// rounded down would let a material show darker than its author derived.
fn f16_at_or_above(v: f32) -> u16 {
    debug_assert!(v >= 0.0, "a floor is a factor: {v}");
    let v = F16x4::saturate_lane(v);
    let nearest = half::f16::from_f32(v);
    if nearest.to_f32() < v {
        nearest.to_bits() + 1
    } else {
        nearest.to_bits()
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::common::span::Span;
    use crate::primitives::identity::effect_id::EffectId;
    use crate::primitives::packed::half_simd::F16x4;
    use crate::shape::paint::capture_params::{CaptureParams, CaptureReach};
    use crate::shape::paint::lowered_material::LoweredMaterial;

    impl LoweredMaterial {
        /// A material through effect 0 that captures, blurred by σ `sigma` logical px, with no
        /// lens or bound, and no input staged.
        pub(crate) fn fixture(sigma: f32) -> Self {
            Self {
                effect: EffectId(0),
                capture: CaptureParams {
                    sigma,
                    reach: CaptureReach::Lens(F16x4::from_lanes([0.0, 0.0, 1.0, 0.0])),
                },
                params: Span::default(),
                input_hash: 0,
                max_luminance: half::f16::MAX.to_bits(),
                min_luminance: 0,
            }
        }
    }
}
