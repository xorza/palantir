//! [`EffectName`]: an effect named inline, typed by its parameters and its kind.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use serde::de;

use crate::primitives::effect::effect_kind::{ColorEffect, EffectKind, EffectTypes};
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::identity::inline_name::InlineName;

/// Which effect to paint, by name: the name is the effect's identity, as [`FontFamily`]'s is a
/// family's. `U` is its parameter struct and `K` its kind, so a shape that names it takes
/// parameters of the right type, which rustc checks. `Copy` and made by a `const fn`, so an app
/// keeps its effects as constants; serializes as its name. Refused: an RAII handle beside a
/// string id, two routes to one effect.
///
/// [`FontFamily`]: crate::FontFamily
pub struct EffectName<U: EffectParams, K: EffectKind = ColorEffect> {
    name: InlineName,
    types: PhantomData<fn() -> (U, K)>,
}

impl<U: EffectParams, K: EffectKind> EffectName<U, K> {
    /// The longest name an effect holds, in bytes.
    pub const MAX_NAME_LEN: usize = InlineName::MAX_LEN;

    /// The effect called `name`. `None` for an empty name or one longer than
    /// [`Self::MAX_NAME_LEN`] bytes.
    pub const fn named(name: &str) -> Option<Self> {
        match InlineName::new(name) {
            Some(name) => Some(Self::from_inline(name)),
            None => None,
        }
    }

    /// This effect's name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub(crate) const fn inline(self) -> InlineName {
        self.name
    }

    /// This name with its types as a run-time value.
    pub(crate) fn typed(self) -> TypedName {
        TypedName {
            name: self.name,
            types: EffectTypes::of::<U, K>(),
        }
    }

    const fn from_inline(name: InlineName) -> Self {
        Self {
            name,
            types: PhantomData,
        }
    }
}

/// An [`EffectName`] with its parameter type and kind as a run-time value, so code that resolves a
/// name of any type compiles once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TypedName {
    pub(crate) name: InlineName,
    pub(crate) types: EffectTypes,
}

impl<U: EffectParams, K: EffectKind> Clone for EffectName<U, K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<U: EffectParams, K: EffectKind> Copy for EffectName<U, K> {}

impl<U: EffectParams, K: EffectKind> PartialEq for EffectName<U, K> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl<U: EffectParams, K: EffectKind> Eq for EffectName<U, K> {}

impl<U: EffectParams, K: EffectKind> Hash for EffectName<U, K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl<U: EffectParams, K: EffectKind> fmt::Debug for EffectName<U, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("EffectName").field(&self.name()).finish()
    }
}

impl<U: EffectParams, K: EffectKind> serde::Serialize for EffectName<U, K> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de, U: EffectParams, K: EffectKind> serde::Deserialize<'de> for EffectName<U, K> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_str(NameVisitor)
            .map(Self::from_inline)
    }
}

/// A visitor, so a borrowed name reads without allocating.
#[derive(Debug)]
struct NameVisitor;

impl de::Visitor<'_> for NameVisitor {
    type Value = InlineName;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an effect name of 1 to 63 bytes")
    }

    fn visit_str<E: de::Error>(self, name: &str) -> Result<Self::Value, E> {
        InlineName::new(name).ok_or_else(|| {
            E::custom(format_args!(
                "an effect name must be 1 to {} bytes, got {}",
                InlineName::MAX_LEN,
                name.len()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use ron::ser;

    use crate::primitives::effect::effect_kind::BackdropEffect;
    use crate::primitives::effect::effect_name::EffectName;

    type Ripple = EffectName<()>;

    /// A name is 1 to 63 bytes: 63 fit, 64 do not, and an empty name names no effect. Serde
    /// carries the name alone and refuses one that does not fit.
    #[test]
    fn a_name_fits_and_serializes_as_itself() {
        const RIPPLE: Ripple = Ripple::named("ripple").expect("the name fits");
        let longest = "e".repeat(Ripple::MAX_NAME_LEN);
        assert_eq!(Ripple::named(&longest).unwrap().name(), longest);
        assert_eq!(Ripple::named(&format!("{longest}e")), None);
        assert_eq!(Ripple::named(""), None);
        assert_eq!(RIPPLE, Ripple::named("ripple").unwrap());
        assert_ne!(RIPPLE, Ripple::named("ripples").unwrap());

        let encoded = ser::to_string(&RIPPLE).expect("serialize");
        assert_eq!(encoded, "\"ripple\"");
        assert_eq!(ron::from_str::<Ripple>(&encoded).expect("parse"), RIPPLE);
        let water: EffectName<(), BackdropEffect> = ron::from_str("\"water\"").expect("parse");
        assert_eq!(water.name(), "water");
        let long = format!("\"{}\"", "e".repeat(64));
        assert!(ron::from_str::<Ripple>(&long).is_err());
    }
}
