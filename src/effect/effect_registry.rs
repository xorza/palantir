//! [`EffectRegistry`]: the effects a host loaded, by name, shared by every window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::effect::contract::{self, ParamsLayout};
use crate::effect::effect_kind::{EffectKind, EffectTypes};
use crate::effect::effect_name::EffectName;
use crate::effect::effect_params::EffectParams;
use crate::effect::effect_source::{EffectSource, ReducedPaint};
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
    image_slots: u8,
    /// What paints in its place for each preference, in [`ReducedBy`]'s order.
    alternatives: [ReducedName; ReducedBy::COUNT],
}

/// A preference an effect's source names an alternative for, in the order one wins over the
/// next where several reduce: what a person needs to read, then to bear, before what a device
/// saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReducedBy {
    Transparency,
    Motion,
    Power,
}

impl ReducedBy {
    const COUNT: usize = 3;
    const ALL: [Self; Self::COUNT] = [Self::Transparency, Self::Motion, Self::Power];

    const fn name(self) -> &'static str {
        match self {
            Self::Transparency => "reduced-transparency",
            Self::Motion => "reduced-motion",
            Self::Power => "power-saver",
        }
    }
}

/// What the system asks to reduce now, which picks the alternative a shape paints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reductions {
    pub(crate) transparency: Reduction,
    pub(crate) motion: Reduction,
    pub(crate) power: Reduction,
}

impl Reductions {
    const fn reduces(self, by: ReducedBy) -> bool {
        match by {
            ReducedBy::Transparency => self.transparency.is_reduced(),
            ReducedBy::Motion => self.motion.is_reduced(),
            ReducedBy::Power => self.power.is_reduced(),
        }
    }
}

/// The effect a shape paints, as [`EffectRegistry::resolve`] finds it: its id, and how many
/// images each draw binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedEffect {
    pub(crate) id: EffectId,
    pub(crate) image_slots: u8,
}

/// [`ReducedPaint`] with the alternative's types checked at the load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReducedName {
    Same,
    Effect(InlineName),
    Nothing,
}

impl ReducedName {
    const fn of<U: EffectParams, K: EffectKind>(paint: ReducedPaint<U, K>) -> Self {
        match paint {
            ReducedPaint::Same => Self::Same,
            ReducedPaint::Effect(other) => Self::Effect(other.inline()),
            ReducedPaint::Nothing => Self::Nothing,
        }
    }
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
    /// When `name`, or an alternative its source names, was loaded before with another parameter
    /// type or kind, or the alternative with other image slots.
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
            // An alternative loaded already is held to the same types and images now, as its
            // shapes pass both; one loaded later is held to them where it paints in place of this
            // one.
            for paint in source.alternatives() {
                if let ReducedPaint::Effect(other) = paint
                    && let Some(loaded) = effects.get(other.inline())
                {
                    assert_types(other.name(), loaded.types, types);
                    assert!(
                        loaded.image_slots == source.image_slots,
                        "effect `{}` binds {} images, and `{}`, which paints in its place, {}",
                        other.name(),
                        loaded.image_slots,
                        name.name(),
                        source.image_slots,
                    );
                }
            }
        }
        let params = ParamsLayout {
            fields: U::FIELDS,
            size: size_of::<U>(),
        };
        let alternatives = source.alternatives().map(ReducedName::of);
        let EffectSource {
            wgsl, image_slots, ..
        } = source;
        let module = contract::check(&wgsl, K::ID, params, image_slots)?;
        let id = self.effects.borrow_mut().insert(
            name.inline(),
            LoadedEffect {
                types,
                image_slots,
                alternatives,
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

    /// The effect a shape that names `name` paints, whose id it records: the effect loaded under
    /// `name`, or the alternative its source names for the first preference `reductions` reduces
    /// that names one, in [`ReducedBy`]'s order; `None` when that is nothing. A move of any of them
    /// records every window again, so the record resolves it each frame it changes.
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
        reductions: Reductions,
    ) -> Option<ResolvedEffect> {
        let id = self
            .id_of(name)
            .unwrap_or_else(|| panic!("no effect is loaded under `{}`", name.name()));
        let effects = self.effects.borrow();
        let resolved = |id: EffectId| ResolvedEffect {
            id,
            image_slots: effects.loaded[id.idx()].image_slots,
        };
        let alternatives = effects.loaded[id.idx()].alternatives;
        let Some((by, alternative)) = ReducedBy::ALL
            .into_iter()
            .zip(alternatives)
            .find(|&(by, alternative)| alternative != ReducedName::Same && reductions.reduces(by))
        else {
            return Some(resolved(id));
        };
        match alternative {
            ReducedName::Same => unreachable!("only an alternative of its own is found"),
            ReducedName::Nothing => None,
            ReducedName::Effect(other) => {
                let other_id = *effects.ids.get(&other).unwrap_or_else(|| {
                    panic!(
                        "no effect is loaded under `{}`, the {} alternative of `{}`",
                        other.as_str(),
                        by.name(),
                        name.name()
                    )
                });
                assert_types(
                    other.as_str(),
                    effects.loaded[other_id.idx()].types,
                    EffectTypes::of::<U, K>(),
                );
                Some(resolved(other_id))
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
    use crate::effect::effect_registry::{EffectRegistry, ReducedBy, ReducedName};

    impl EffectRegistry {
        /// The power-saver alternative loaded under `name`; `None` when nothing is loaded.
        pub(crate) fn power_saver_of<U: EffectParams, K: EffectKind>(
            &self,
            name: EffectName<U, K>,
        ) -> Option<ReducedName> {
            let effects = self.effects.borrow();
            effects
                .get(name.inline())
                .map(|loaded| loaded.alternatives[ReducedBy::Power as usize])
        }
    }
}
