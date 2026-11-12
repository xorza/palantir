//! [`MaterialEffect`]: the backdrop effect a material wears, with its parameters, held inline.

use std::fmt;
use std::ptr;

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeStruct;

use crate::primitives::effect::effect_kind::BackdropEffect;
use crate::primitives::effect::effect_name::{EffectName, ErasedEffectName};
use crate::primitives::effect::effect_params::EffectParams;

/// The backdrop effect a [`Material`](crate::Material) wears, by name, with the parameters it
/// hands it: up to [`Self::MAX_LANES`] f32 lanes, in the order the effect's `Params` struct
/// declares its members, held inline, so a material stays a plain value a theme saves and a tween
/// blends. [`Self::new`] lays out the effect's own parameter type, which must be f32 lanes alone,
/// so the name and its parameters agree by construction; a theme file names the effect and lists
/// its lanes in order, as Flutter's `FragmentShader.setFloat` and Skia's runtime-effect uniforms
/// take them, and [`Ui::set_theme`](crate::Ui::set_theme) holds their count to the loaded effect's.
///
/// Refused: the name and the parameters as two fields, which a caller could set apart; an
/// allocation of the bytes, which every material built in a frame would pay; any parameter type,
/// which a blend could not read and a theme file could not hold; more lanes, which every paint
/// would carry. A larger or integer parameter struct paints through an
/// [`EffectPaint`](crate::widget::EffectPaint).
#[derive(Clone, Copy, PartialEq)]
#[must_use]
pub struct MaterialEffect {
    name: ErasedEffectName,
    /// The lanes, those past `count` 0, so equality reads the whole array.
    lanes: [f32; Self::MAX_LANES],
    count: u8,
}

impl MaterialEffect {
    /// The most f32 lanes a material hands its effect: the glass takes 7.
    pub const MAX_LANES: usize = 8;

    /// The effect `effect` reading `params`, laid out as lanes, each f32 in declaration order.
    ///
    /// # Panics
    ///
    /// At compile time, unless `U`'s fields are f32 lanes that span it whole and fit
    /// [`Self::MAX_LANES`]: a struct of an integer is no material's.
    ///
    /// ```compile_fail,E0080
    /// # use palantir::{BackdropEffect, EffectName, EffectParams, MaterialEffect};
    /// #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, EffectParams)]
    /// #[repr(C)]
    /// struct Steps {
    ///     count: u32,
    /// }
    /// let steps = EffectName::<Steps, BackdropEffect>::named("steps").unwrap();
    /// let _ = MaterialEffect::new(steps, Steps { count: 3 });
    /// ```
    pub const fn new<U: EffectParams>(effect: EffectName<U, BackdropEffect>, params: U) -> Self {
        let count = const {
            match Self::lanes_for::<U>() {
                Some(count) => count,
                None => panic!(
                    "a material's parameters are f32 lanes alone, which span the struct and fit 8"
                ),
            }
        };
        let first = ptr::from_ref(&params).cast::<f32>();
        let mut lanes = [0.0; Self::MAX_LANES];
        let mut lane = 0;
        while lane < count {
            // SAFETY: `lanes_for` holds `U` to `count` whole f32s, and a `Pod` holds every bit
            // pattern; the read is unaligned, as a `Pod` may align below an f32.
            lanes[lane] = unsafe { first.add(lane).read_unaligned() };
            lane += 1;
        }
        Self {
            name: effect.erased(),
            lanes,
            count: count as u8,
        }
    }

    /// How many f32 lanes `U` lays out; `None` unless its fields are f32 lanes alone, which span
    /// it whole, so every byte is a listed lane, and fit [`Self::MAX_LANES`].
    pub(crate) const fn lanes_for<U: EffectParams>() -> Option<usize> {
        let size = size_of::<U>();
        let mut listed = 0;
        let mut field = 0;
        while field < U::FIELDS.len() {
            let ty = U::FIELDS[field].ty;
            if !ty.is_f32() {
                return None;
            }
            listed += ty.size();
            field += 1;
        }
        if listed == size && size / 4 <= Self::MAX_LANES {
            Some(size / 4)
        } else {
            None
        }
    }

    /// The effect's name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The parameters' lanes, in the order the effect declares them.
    pub fn params(&self) -> &[f32] {
        &self.lanes[..usize::from(self.count)]
    }

    /// The effect's name, as the registry keys it.
    pub(crate) const fn erased_name(&self) -> ErasedEffectName {
        self.name
    }

    /// How many lanes the parameters hold.
    pub(crate) const fn count(&self) -> u8 {
        self.count
    }

    /// Whether `other` names the same effect over as many lanes, so the two blend lane by lane.
    pub(crate) fn blends_with(&self, other: &Self) -> bool {
        self.name == other.name && self.count == other.count
    }

    /// The lanes as the bytes the effect reads.
    pub(crate) fn bytes(&self) -> &[u8] {
        bytemuck::cast_slice(self.params())
    }

    /// The lanes as `U`, which the loaded effect takes, its lane count checked against these.
    pub(crate) fn read<U: EffectParams>(&self) -> U {
        bytemuck::pod_read_unaligned(self.bytes())
    }

    /// Each lane mapped by `f`, the name and count kept: the spring arithmetic.
    pub(crate) fn map(self, mut f: impl FnMut(f32) -> f32) -> Self {
        let mut lanes = self.lanes;
        for lane in &mut lanes[..usize::from(self.count)] {
            *lane = f(*lane);
        }
        Self { lanes, ..self }
    }

    /// Each lane of `self` and `other` combined by `f`, the name and count `self`'s; a lane
    /// `other` lacks reads 0.
    pub(crate) fn zip(self, other: &Self, mut f: impl FnMut(f32, f32) -> f32) -> Self {
        let mut lanes = self.lanes;
        for (lane, other) in lanes[..usize::from(self.count)].iter_mut().zip(other.lanes) {
            *lane = f(*lane, other);
        }
        Self { lanes, ..self }
    }

    /// Whether every lane is finite.
    pub(crate) const fn is_valid(&self) -> bool {
        let mut lane = 0;
        while lane < self.count as usize {
            if !self.lanes[lane].is_finite() {
                return false;
            }
            lane += 1;
        }
        true
    }

    pub(crate) fn has_nan(&self) -> bool {
        self.params().iter().any(|lane| lane.is_nan())
    }
}

impl fmt::Debug for MaterialEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaterialEffect")
            .field("name", &self.name())
            .field("params", &self.params())
            .finish()
    }
}

/// `(name: "palantir.frosted", params: [1.8, 0.04])`, the parameters left out where the effect
/// takes none.
impl serde::Serialize for MaterialEffect {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = if self.count == 0 { 1 } else { 2 };
        let mut file = serializer.serialize_struct("MaterialEffect", fields)?;
        file.serialize_field("name", self.name())?;
        if self.count == 0 {
            file.skip_field("params")?;
        } else {
            file.serialize_field("params", self.params())?;
        }
        file.end()
    }
}

/// The name as an [`EffectName`] reads it, and at most [`MaterialEffect::MAX_LANES`] finite
/// numbers.
impl<'de> serde::Deserialize<'de> for MaterialEffect {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            name: EffectName<(), BackdropEffect>,
            #[serde(default)]
            params: Lanes,
        }
        let File { name, params } = File::deserialize(deserializer)?;
        Ok(Self {
            name: name.erased(),
            lanes: params.lanes,
            count: params.count,
        })
    }
}

/// A file's lanes, as [`LanesVisitor`] reads them.
#[derive(Debug, Default)]
struct Lanes {
    lanes: [f32; MaterialEffect::MAX_LANES],
    count: u8,
}

impl<'de> serde::Deserialize<'de> for Lanes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(LanesVisitor)
    }
}

#[derive(Debug)]
struct LanesVisitor;

impl<'de> Visitor<'de> for LanesVisitor {
    type Value = Lanes;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a list of at most {} finite numbers",
            MaterialEffect::MAX_LANES
        )
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut lanes = Lanes::default();
        while let Some(lane) = seq.next_element::<f32>()? {
            let idx = usize::from(lanes.count);
            if idx == MaterialEffect::MAX_LANES {
                return Err(de::Error::invalid_length(idx + 1, &self));
            }
            if !lane.is_finite() {
                return Err(de::Error::invalid_value(
                    de::Unexpected::Float(f64::from(lane)),
                    &"a finite number",
                ));
            }
            lanes.lanes[idx] = lane;
            lanes.count += 1;
        }
        Ok(lanes)
    }
}
