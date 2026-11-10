//! A material lowered: the effect it paints through, what it reads and captures, its input's
//! words in the record store, and its luminance ceiling and floor.

use std::hash::{self, Hasher as _};

use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::effect::effect_registry::EffectRegistry;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::reduced_backdrop::BackdropMode;
use crate::primitives::reduction::Reductions;
use crate::shape::paint::capture_params::{CaptureParams, CaptureReach, EffectBackdrop};
use crate::shape::record_store::RecordStore;

/// What a box's material reads this record, as the reductions decide it, with the effect the
/// registry resolved its name to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MaterialRead {
    pub(crate) effect: EffectId,
    pub(crate) mode: BackdropMode,
}

impl MaterialRead {
    /// What `material` reads under `reductions`, its effect resolved in `effects`; `None` where it
    /// paints nothing: an alternative of nothing for reduced motion or power, or
    /// [`ReducedBackdrop::Nothing`](crate::ReducedBackdrop::Nothing) while transparency is reduced, where the box's fill paints as
    /// it is.
    ///
    /// # Panics
    ///
    /// When no effect is loaded under the material's name, whatever is reduced: a name nothing
    /// loaded is a logic error in every mode.
    #[track_caller]
    pub(crate) fn resolve(
        material: &Material,
        effects: &EffectRegistry,
        reductions: Reductions,
    ) -> Option<Self> {
        let resolved = effects.resolve(material.effect.typed(), reductions)?;
        debug_assert_eq!(
            resolved.image_slots, 0,
            "a material effect's load refuses image slots"
        );
        Some(Self {
            effect: resolved.id,
            mode: material
                .reduced_transparency
                .mode(reductions.transparency)?,
        })
    }
}

/// A [`Material`] as a record keeps it: the effect and what it reads, a capture whose reach is
/// its lens, as a backdrop `EffectPaint`'s reach is its own, the first of the input's words in the
/// record store and their hash, and the luminance bounds. `max_luminance` is the ceiling's f16
/// bits, rounded down so the material never shows brighter than its author allowed, and 65504 for
/// none; `min_luminance` the floor's, rounded up so it never shows darker, and 0 for none. Packed
/// for the shape record and the chrome row that hold it: 40 B.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LoweredMaterial {
    /// The hash of the input's bytes, as their words are frame-local.
    input_hash: u64,
    effect: EffectId,
    pub(crate) backdrop: EffectBackdrop,
    /// The input's words in the record store, which the record pass that staged them keeps.
    pub(crate) params: Span,
    max_luminance: u16,
    min_luminance: u16,
}

impl LoweredMaterial {
    /// `material` reading `read`, its input staged in `store`. [`Material::validate`] holds an
    /// authored material within its kinds, and an animated chrome's tween may carry it past them,
    /// as [`Easing::OutBack`](crate::Easing::OutBack) does past an end: here, where it starts to
    /// paint, each value coerces into its kind and saturates at the lanes.
    pub(crate) fn lower(store: &mut RecordStore, material: &Material, read: MaterialRead) -> Self {
        let mut material = *material;
        material.coerce();
        let bytes = bytemuck::bytes_of(&material.input);
        let mut h = Hasher::new();
        h.pod_slice(bytes);
        let lens = material.input.lens;
        let capture = CaptureParams {
            sigma: F16x4::saturate_lane(material.blur),
            reach: CaptureReach::Lens(F16x4::from_lanes_saturating([
                lens.bezel,
                lens.thickness,
                lens.refractive_index,
                lens.dispersion,
            ])),
        };
        Self {
            effect: read.effect,
            backdrop: EffectBackdrop::of(read.mode, capture),
            params: store.stage_effect_params(bytes),
            input_hash: h.finish(),
            max_luminance: material
                .max_luminance
                .map_or(half::f16::MAX.to_bits(), f16_at_or_below),
            min_luminance: material.min_luminance.map_or(0, f16_at_or_above),
        }
    }

    /// The highest relative luminance the material shows: its ceiling, or 65504 with none.
    pub(crate) fn max_luminance(self) -> f32 {
        half::f16::from_bits(self.max_luminance).to_f32()
    }

    /// The lowest relative luminance the material shows: its floor, or 0 with none.
    pub(crate) fn min_luminance(self) -> f32 {
        half::f16::from_bits(self.min_luminance).to_f32()
    }

    /// The effect it paints through.
    pub(crate) const fn effect(self) -> EffectId {
        self.effect
    }

    /// What it captures where it reads what is painted under the box, a backdrop the cascade
    /// lists; `None` over the ground.
    pub(crate) const fn capture(self) -> Option<CaptureParams> {
        self.backdrop.capture()
    }

    /// Whether it reads what is painted under the box.
    pub(crate) const fn captures(self) -> bool {
        self.capture().is_some()
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

/// Everything but the frame-local `params`, whose bytes `input_hash` stands for.
impl hash::Hash for LoweredMaterial {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.effect.0);
        self.backdrop.hash(state);
        state.write_u64(self.input_hash);
        state.write_u16(self.max_luminance);
        state.write_u16(self.min_luminance);
    }
}

impl NanCheck for LoweredMaterial {
    fn has_nan(&self) -> bool {
        self.backdrop.has_nan()
            || half::f16::from_bits(self.max_luminance).is_nan()
            || half::f16::from_bits(self.min_luminance).is_nan()
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::common::span::Span;
    use crate::primitives::identity::effect_id::EffectId;
    use crate::primitives::packed::half_simd::F16x4;
    use crate::shape::paint::capture_params::{CaptureParams, CaptureReach, EffectBackdrop};
    use crate::shape::paint::lowered_material::LoweredMaterial;

    impl LoweredMaterial {
        /// A material through effect 0 that captures, blurred by σ `sigma` logical px, with no
        /// lens or bound, and no input staged.
        pub(crate) fn fixture(sigma: f32) -> Self {
            Self {
                effect: EffectId(0),
                backdrop: EffectBackdrop::Capture(CaptureParams {
                    sigma,
                    reach: CaptureReach::Lens(F16x4::from_lanes([0.0, 0.0, 1.0, 0.0])),
                }),
                params: Span::default(),
                input_hash: 0,
                max_luminance: half::f16::MAX.to_bits(),
                min_luminance: 0,
            }
        }
    }
}
