//! [`EffectRegistry`]: the effects a host loaded, by name, shared by every window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::effect::contract::{self, ParamsLayout};
use crate::effect::effect_kind::{EffectKind, EffectTypes};
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::EffectParams;
use crate::effect::effect_source::{EffectSource, PowerSaverPaint};
use crate::effect::effect_store::EffectStore;
use crate::effect::error::EffectError;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::identity::inline_name::InlineName;
use crate::primitives::reduction::Reduction;

/// Every effect loaded under a name, for the app's life, as a font is: loading the same name again
/// replaces it, and a refused load leaves the one before. A name keeps the parameter type and
/// kind it first loaded with; using it with others is a logic error that panics with both. Each
/// load goes to the [`EffectStore`] the host built the registry over, which compiles it; a
/// deviceless recorder's registry has none.
#[derive(Debug, Default)]
pub(crate) struct EffectRegistry {
    /// Fixed at construction: the host builds the store before any recorder exists.
    store: Option<Rc<dyn EffectStore>>,
    effects: RefCell<Effects>,
    /// Moved by every load: a reload changes an effect's pixels and no shape's hash, as a font
    /// load does, so a frame that paints under a moved epoch repaints whole.
    epoch: Cell<u32>,
}

/// The loaded effects, each at its [`EffectId`], and the id each name holds.
#[derive(Debug, Default)]
struct Effects {
    ids: FxHashMap<InlineName, EffectId>,
    loaded: Vec<LoadedEffect>,
}

/// What a loaded effect keeps on the CPU: its types and what its source asked for. Its module
/// lives in the store, which compiles it.
#[derive(Debug)]
struct LoadedEffect {
    types: EffectTypes,
    #[expect(
        dead_code,
        reason = "a shape binds this many images (proposal batch 7)"
    )]
    image_slots: u8,
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
    /// A registry whose effects compile in `store`, the one the host's backend draws with.
    pub(crate) fn new<S: EffectStore + 'static>(store: Rc<S>) -> Self {
        Self {
            store: Some(store),
            effects: RefCell::default(),
            epoch: Cell::default(),
        }
    }

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
            if let Some(loaded) = effects.get(name.inline()) {
                assert_types(name.name(), loaded.types, types);
            }
            // An alternative loaded already is held to the same types now; one loaded later is
            // held to them where it paints in place of this one.
            if let PowerSaverPaint::Effect(other) = source.power_saver
                && let Some(loaded) = effects.get(other.inline())
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
        let id = self.effects.borrow_mut().insert(
            name.inline(),
            LoadedEffect {
                types,
                image_slots,
                power_saver,
            },
        );
        if let Some(store) = &self.store {
            store.load(id, &module);
        }
        self.epoch.set(self.epoch.get().wrapping_add(1));
        Ok(())
    }

    /// The count of successful loads, wrapping, which every window's frame baselines carry.
    pub(crate) const fn epoch(&self) -> u32 {
        self.epoch.get()
    }

    /// Whether an effect is loaded under `name`.
    ///
    /// # Panics
    ///
    /// When it was loaded with another parameter type or kind.
    pub(crate) fn contains<U: EffectParams, K: EffectKind>(&self, name: EffectName<U, K>) -> bool {
        self.id_of(name).is_some()
    }

    /// The id of the effect a shape that names `name` paints, which it records: the effect loaded
    /// under `name`, or while `power` is reduced the alternative its source names; `None` when
    /// that is nothing. A power move records every window again, so the record resolves it each
    /// frame it changes.
    ///
    /// # Panics
    ///
    /// When no effect is loaded under `name`, or under the alternative it paints in place of it,
    /// or one is loaded with another parameter type or kind: a shape that names an effect nothing
    /// loaded is a logic error, as painting nothing would be quietly wrong.
    #[track_caller]
    pub(crate) fn resolve<U: EffectParams, K: EffectKind>(
        &self,
        name: EffectName<U, K>,
        power: Reduction,
    ) -> Option<EffectId> {
        let id = self
            .id_of(name)
            .unwrap_or_else(|| panic!("no effect is loaded under `{}`", name.name()));
        if !power.is_reduced() {
            return Some(id);
        }
        let effects = self.effects.borrow();
        match effects.loaded[id.idx()].power_saver {
            PowerSaverName::Same => Some(id),
            PowerSaverName::Nothing => None,
            PowerSaverName::Effect(other) => {
                let other_id = *effects.ids.get(&other).unwrap_or_else(|| {
                    panic!(
                        "no effect is loaded under `{}`, the power-saver alternative of `{}`",
                        other.as_str(),
                        name.name()
                    )
                });
                assert_types(
                    other.as_str(),
                    effects.loaded[other_id.idx()].types,
                    EffectTypes::of::<U, K>(),
                );
                Some(other_id)
            }
        }
    }

    #[track_caller]
    fn id_of<U: EffectParams, K: EffectKind>(&self, name: EffectName<U, K>) -> Option<EffectId> {
        let effects = self.effects.borrow();
        let id = *effects.ids.get(&name.inline())?;
        assert_types(
            name.name(),
            effects.loaded[id.idx()].types,
            EffectTypes::of::<U, K>(),
        );
        Some(id)
    }
}

impl Effects {
    fn get(&self, name: InlineName) -> Option<&LoadedEffect> {
        self.ids.get(&name).map(|id| &self.loaded[id.idx()])
    }

    /// Hold `effect` under `name`, at the id the name holds, or at a new one for a new name.
    fn insert(&mut self, name: InlineName, effect: LoadedEffect) -> EffectId {
        let next = EffectId(u32::try_from(self.loaded.len()).expect("fewer than 2³² effects"));
        let id = *self.ids.entry(name).or_insert(next);
        if id == next {
            self.loaded.push(effect);
        } else {
            self.loaded[id.idx()] = effect;
        }
        id
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
            effects.get(name.inline()).map(|loaded| loaded.power_saver)
        }
    }
}
