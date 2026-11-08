//! [`EffectRegistry`]: the effects a host loaded, by name, shared by every window.

use std::cell::RefCell;

use rustc_hash::FxHashMap;

use crate::effect::contract::{self, ParamsLayout};
use crate::effect::effect_kind::{EffectKind, EffectTypes};
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::EffectParams;
use crate::effect::effect_source::{EffectSource, PowerSaverPaint};
use crate::effect::error::EffectError;
use crate::primitives::identity::inline_name::InlineName;

/// Every effect loaded under a name, for the app's life, as a font is: loading the same name again
/// replaces it, and a refused load leaves the one before. A name keeps the parameter type and
/// kind it first loaded with; using it with others is a logic error that panics with both.
#[derive(Debug, Default)]
pub(crate) struct EffectRegistry {
    effects: RefCell<FxHashMap<InlineName, LoadedEffect>>,
}

/// One loaded effect: its types, its validated module, which batch 5's pipelines compile, and
/// what its source asked for.
#[derive(Debug)]
struct LoadedEffect {
    types: EffectTypes,
    #[expect(
        dead_code,
        reason = "the effect pipelines compile it (proposal batch 5)"
    )]
    module: naga::Module,
    #[expect(
        dead_code,
        reason = "a shape binds this many images (proposal batch 7)"
    )]
    image_slots: u8,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "a reduced-power frame paints this (proposal batch 6)"
        )
    )]
    power_saver: PowerSaverName,
}

/// [`PowerSaverPaint`] with the alternative's types checked at the load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PowerSaverName {
    Same,
    Effect(InlineName),
    Nothing,
}

impl EffectRegistry {
    /// Load `source` under `name`, replacing the effect loaded under it before.
    ///
    /// # Panics
    ///
    /// When `name`, or the power-saver alternative its source names, was loaded before with
    /// another parameter type or kind.
    pub(crate) fn load<U: EffectParams, K: EffectKind>(
        &self,
        name: EffectName<U, K>,
        source: EffectSource<U, K>,
    ) -> Result<(), EffectError> {
        let types = EffectTypes::of::<U, K>();
        {
            let effects = self.effects.borrow();
            if let Some(loaded) = effects.get(&name.inline()) {
                assert_types(name.name(), loaded.types, types);
            }
            // An alternative loaded already is held to the same types now; one loaded later is
            // held to them where it paints in place of this one.
            if let PowerSaverPaint::Effect(other) = source.power_saver
                && let Some(loaded) = effects.get(&other.inline())
            {
                assert_types(other.name(), loaded.types, types);
            }
        }
        let params = ParamsLayout {
            fields: U::FIELDS,
            size: size_of::<U>(),
        };
        let EffectSource {
            wgsl,
            image_slots,
            power_saver,
        } = source;
        let module = contract::check(&wgsl, params, image_slots)?;
        let power_saver = match power_saver {
            PowerSaverPaint::Same => PowerSaverName::Same,
            PowerSaverPaint::Effect(other) => PowerSaverName::Effect(other.inline()),
            PowerSaverPaint::Nothing => PowerSaverName::Nothing,
        };
        self.effects.borrow_mut().insert(
            name.inline(),
            LoadedEffect {
                types,
                module,
                image_slots,
                power_saver,
            },
        );
        Ok(())
    }

    /// Whether an effect is loaded under `name`.
    ///
    /// # Panics
    ///
    /// When it was loaded with another parameter type or kind.
    pub(crate) fn contains<U: EffectParams, K: EffectKind>(&self, name: EffectName<U, K>) -> bool {
        let effects = self.effects.borrow();
        let Some(loaded) = effects.get(&name.inline()) else {
            return false;
        };
        assert_types(name.name(), loaded.types, EffectTypes::of::<U, K>());
        true
    }
}

#[track_caller]
fn assert_types(name: &str, loaded: EffectTypes, named: EffectTypes) {
    assert!(
        loaded == named,
        "effect `{name}` was loaded as {loaded} and is named here as {named}"
    );
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::effect::effect_kind::EffectKind;
    use crate::effect::effect_name::EffectName;
    use crate::effect::effect_params::EffectParams;
    use crate::effect::effect_registry::{EffectRegistry, PowerSaverName};

    impl EffectRegistry {
        /// The power-saver alternative loaded under `name`; `None` when nothing is loaded.
        pub(crate) fn power_saver_of<U: EffectParams, K: EffectKind>(
            &self,
            name: EffectName<U, K>,
        ) -> Option<PowerSaverName> {
            let effects = self.effects.borrow();
            effects.get(&name.inline()).map(|loaded| loaded.power_saver)
        }
    }
}
