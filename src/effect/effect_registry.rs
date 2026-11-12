//! [`EffectRegistry`]: the effects a host loaded, by name, shared by every window.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::effect::contract::{self, ContractSpec, ParamsLayout};
use crate::effect::effect_source::{EffectBuild, EffectSource, ReducedPaint};
use crate::effect::effect_store::EffectStore;
use crate::effect::error::EffectError;
use crate::effect::stock::{self, StockSources};
use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::effect::effect_kind::{EffectKind, EffectTypes};
use crate::primitives::effect::effect_name::{CheckedEffectName, EffectName, ErasedEffectName};
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::material_effect::MaterialEffect;
use crate::primitives::paint::material::material_reach::MaterialReach;
use crate::primitives::reduction::Reductions;

/// Every effect loaded under a name, for the app's life, as a font is: loading the same name again
/// replaces it, and a refused load leaves the one before. A name keeps the parameter type and
/// kind it first loaded with; using it with others is a logic error that panics with both. Each
/// load goes to the [`EffectStore`] the host built the registry over, which compiles it; a
/// deviceless recorder's registry has none. It holds palantir's stock effects
/// (`crate::effect::stock`) from its start, loaded as an app's load, under names no app load
/// takes.
#[derive(Debug)]
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
    ids: FxHashMap<ErasedEffectName, EffectId>,
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
    /// What a material over it reads of it; `None` for an effect no material wears.
    material: Option<MaterialFacts>,
}

/// What a [`Material`] over an effect needs of it, the parameters' type erased: how many lanes
/// they hold, and how far past its box it reads from them.
struct MaterialFacts {
    lanes: u8,
    reach: Box<dyn Fn(&MaterialEffect) -> MaterialReach>,
}

impl fmt::Debug for MaterialFacts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaterialFacts")
            .field("lanes", &self.lanes)
            .finish_non_exhaustive()
    }
}

impl MaterialFacts {
    /// What a material over an effect of `source` reads of it; `None` unless it is a backdrop
    /// effect that binds no image, over parameters a [`MaterialEffect`] holds
    /// ([`MaterialEffect::lanes_for`]), as a box hands its material no image.
    fn of<U: EffectParams, K: EffectKind>(source: &EffectSource<U, K>) -> Option<Self> {
        if K::ID != KindId::Backdrop || source.image_slots > 0 {
            return None;
        }
        let lanes = MaterialEffect::lanes_for::<U>()?;
        let reach = source.reach;
        Some(Self {
            lanes: u8::try_from(lanes).expect("at most 8 lanes"),
            reach: Box::new(move |effect: &MaterialEffect| reach(&effect.read::<U>())),
        })
    }
}

/// How a [`MaterialEffect`] fits the effect loaded under its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaterialFit {
    /// Nothing is loaded under the name.
    Unloaded,
    /// The effect loaded is no material's: a color effect, one that binds images, or one over
    /// other than f32 lanes that fit.
    NotMaterial,
    /// A material's, over this many lanes.
    Lanes(u8),
}

/// The effect a material paints, as [`EffectRegistry::resolve_material`] finds it: its id, and
/// how far past its box it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedMaterial {
    pub(crate) id: EffectId,
    pub(crate) reach: MaterialReach,
}

/// A preference an effect's source names an alternative for, in the order one wins over the
/// next where both reduce: what a person needs to bear before what a device saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReducedBy {
    Motion,
    Power,
}

impl ReducedBy {
    const COUNT: usize = 2;
    const ALL: [Self; Self::COUNT] = [Self::Motion, Self::Power];

    const fn name(self) -> &'static str {
        match self {
            Self::Motion => "reduced-motion",
            Self::Power => "power-saver",
        }
    }

    /// Whether `reductions` reduces this preference.
    const fn is_reduced_in(self, reductions: Reductions) -> bool {
        match self {
            Self::Motion => reductions.motion.is_reduced(),
            Self::Power => reductions.power.is_reduced(),
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
    Effect(ErasedEffectName),
    Nothing,
}

impl ReducedName {
    const fn of<U: EffectParams, K: EffectKind>(paint: ReducedPaint<U, K>) -> Self {
        match paint {
            ReducedPaint::Same => Self::Same,
            ReducedPaint::Effect(other) => Self::Effect(other.erased()),
            ReducedPaint::Nothing => Self::Nothing,
        }
    }
}

/// A deviceless recorder's registry: the stock effects, compiled nowhere.
impl Default for EffectRegistry {
    fn default() -> Self {
        Self::with_store(None)
    }
}

impl EffectRegistry {
    /// A registry whose effects compile in `store`, the one the host's backend draws with.
    pub(crate) fn new<S: EffectStore + 'static>(store: Rc<S>) -> Self {
        Self::with_store(Some(store))
    }

    fn with_store(store: Option<Rc<dyn EffectStore>>) -> Self {
        let registry = Self {
            store,
            effects: RefCell::default(),
            epoch: Cell::default(),
        };
        let stock = stock::sources();
        for (name, source) in stock.press {
            registry
                .insert(name, source)
                .expect("a stock effect meets the contract");
        }
        let StockSources { frosted, glass, .. } = stock;
        registry
            .insert(frosted.0, frosted.1)
            .expect("a stock effect meets the contract");
        registry
            .insert(glass.0, glass.1)
            .expect("a stock effect meets the contract");
        registry
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
        if name.name().starts_with(stock::RESERVED_PREFIX) {
            return Err(EffectError::ReservedName);
        }
        self.insert(name, source)
    }

    /// [`Self::load`] under any name, the reserved ones included.
    fn insert<U: EffectParams, K: EffectKind>(
        &self,
        name: EffectName<U, K>,
        source: EffectSource<U, K>,
    ) -> Result<(), EffectError> {
        let types = EffectTypes::of::<U, K>();
        {
            let effects = self.effects.borrow();
            if let Some(loaded) = effects.get(name.erased()) {
                assert_types(name.name(), loaded.types, types);
            }
            // An alternative loaded already is held to the same types and images now, as its
            // shapes pass both; one loaded later is held to them where it paints in place of this
            // one.
            for paint in source.alternatives() {
                if let ReducedPaint::Effect(other) = paint
                    && let Some(loaded) = effects.get(other.erased())
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
        let material = MaterialFacts::of(&source);
        let EffectSource {
            wgsl,
            image_slots,
            build,
            ..
        } = source;
        let module = contract::check(
            &wgsl,
            ContractSpec {
                kind: K::ID,
                params,
                image_slots,
            },
        )?;
        let id = self.effects.borrow_mut().insert(
            name.erased(),
            LoadedEffect {
                types,
                image_slots,
                alternatives,
                material,
            },
        );
        if let Some(store) = &self.store {
            store.register(id, &module);
            if build == EffectBuild::Now {
                store.want(id);
            }
        }
        self.epoch.set(self.epoch.get().wrapping_add(1));
        Ok(())
    }

    /// The types the effect under `name` loaded with; `None` when nothing is loaded under it. A
    /// check of a name that untrusted data holds, as a theme file's, which may name anything.
    pub(crate) fn types_of(&self, name: ErasedEffectName) -> Option<EffectTypes> {
        self.effects.borrow().get(name).map(|loaded| loaded.types)
    }

    /// Want the effect loaded under `name` built before the next frame draws, with each
    /// alternative its source names that is loaded now, as a theme that names it does: no frame
    /// after builds them again, whatever it reduces. An alternative loaded later builds where it
    /// first paints.
    ///
    /// # Panics
    ///
    /// When no effect is loaded under `name`, or one with another parameter type or kind: a theme
    /// set checks its names first ([`Self::types_of`]).
    #[track_caller]
    pub(crate) fn want<U: EffectParams, K: EffectKind>(&self, name: EffectName<U, K>) {
        let id = self
            .id_of(name.checked())
            .unwrap_or_else(|| panic!("no effect is loaded under `{}`", name.name()));
        self.want_id(id);
    }

    /// Want `id` built, with each alternative its source names that is loaded now.
    fn want_id(&self, id: EffectId) {
        let Some(store) = &self.store else {
            return;
        };
        store.want(id);
        let effects = self.effects.borrow();
        for alternative in effects.loaded[id.idx()].alternatives {
            if let ReducedName::Effect(other) = alternative
                && let Some(&other_id) = effects.ids.get(&other)
            {
                store.want(other_id);
            }
        }
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
        self.id_of(name.checked()).is_some()
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
    pub(crate) fn resolve(
        &self,
        name: CheckedEffectName,
        reductions: Reductions,
    ) -> Option<ResolvedEffect> {
        let id = self
            .id_of(name)
            .unwrap_or_else(|| panic!("no effect is loaded under `{}`", name.name.as_str()));
        let painted = self.painted(id, name.name, reductions)?;
        Some(ResolvedEffect {
            id: painted,
            image_slots: self.effects.borrow().loaded[painted.idx()].image_slots,
        })
    }

    /// How `effect` fits the effect loaded under its name: a check of a name a theme file holds,
    /// which may name anything.
    pub(crate) fn material_fit(&self, effect: MaterialEffect) -> MaterialFit {
        match self.effects.borrow().get(effect.erased_name()) {
            None => MaterialFit::Unloaded,
            Some(LoadedEffect { material: None, .. }) => MaterialFit::NotMaterial,
            Some(LoadedEffect {
                material: Some(facts),
                ..
            }) => MaterialFit::Lanes(facts.lanes),
        }
    }

    /// Want the effect `effect` names built before the next frame draws, as [`Self::want`] does
    /// for a typed name.
    ///
    /// # Panics
    ///
    /// When nothing is loaded under the name: a theme set checks its materials first
    /// ([`Self::material_fit`]).
    #[track_caller]
    pub(crate) fn want_material(&self, effect: MaterialEffect) {
        let id = *self
            .effects
            .borrow()
            .ids
            .get(&effect.erased_name())
            .unwrap_or_else(|| panic!("no effect is loaded under `{}`", effect.name()));
        self.want_id(id);
    }

    /// The effect `material` paints, as [`Self::resolve`] finds a typed name's, and how far past
    /// its box it reads, from its parameters, as the effect painted declares.
    ///
    /// # Panics
    ///
    /// When nothing is loaded under the material's effect, or under the alternative it paints in
    /// place of it, or the effect is no material's, or takes another count of lanes than the
    /// material holds: a material that cannot paint is a logic error, as painting nothing would
    /// be quietly wrong.
    #[track_caller]
    pub(crate) fn resolve_material(
        &self,
        material: &Material,
        reductions: Reductions,
    ) -> Option<ResolvedMaterial> {
        let name = material.effect.erased_name();
        let id = {
            let effects = self.effects.borrow();
            let id = *effects
                .ids
                .get(&name)
                .unwrap_or_else(|| panic!("no effect is loaded under `{}`", name.as_str()));
            let want = Self::facts(&effects, id, name).lanes;
            let got = material.effect.count();
            assert!(
                want == got,
                "effect `{}` takes {want} parameter lanes, and its material holds {got}",
                name.as_str()
            );
            id
        };
        let painted = self.painted(id, name, reductions)?;
        let effects = self.effects.borrow();
        let facts = Self::facts(&effects, painted, name);
        Some(ResolvedMaterial {
            id: painted,
            reach: (facts.reach)(&material.effect),
        })
    }

    /// What a material over the effect at `id` reads of it, the effect a material over `name`
    /// paints.
    ///
    /// # Panics
    ///
    /// When the effect is no material's, as an alternative loaded after the effect that names it
    /// may be.
    #[track_caller]
    fn facts(effects: &Effects, id: EffectId, name: ErasedEffectName) -> &MaterialFacts {
        effects.loaded[id.idx()].material.as_ref().unwrap_or_else(|| {
            panic!(
                "a material over `{}` paints effect {}, which is no material's: a backdrop effect \
                 over at most {} f32 lanes that binds no image",
                name.as_str(),
                id.0,
                MaterialEffect::MAX_LANES
            )
        })
    }

    /// The effect a shape that names `id`, loaded under `name`, paints: `id`, or the alternative
    /// its source names for the first preference `reductions` reduces that names one, in
    /// [`ReducedBy`]'s order; `None` when that is nothing.
    ///
    /// # Panics
    ///
    /// When no effect is loaded under the alternative, or one with other types.
    #[track_caller]
    fn painted(
        &self,
        id: EffectId,
        name: ErasedEffectName,
        reductions: Reductions,
    ) -> Option<EffectId> {
        let effects = self.effects.borrow();
        let loaded = &effects.loaded[id.idx()];
        let Some((by, alternative)) =
            ReducedBy::ALL
                .into_iter()
                .zip(loaded.alternatives)
                .find(|&(by, alternative)| {
                    alternative != ReducedName::Same && by.is_reduced_in(reductions)
                })
        else {
            return Some(id);
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
                        name.as_str()
                    )
                });
                assert_types(
                    other.as_str(),
                    effects.loaded[other_id.idx()].types,
                    loaded.types,
                );
                Some(other_id)
            }
        }
    }

    #[track_caller]
    fn id_of(&self, name: CheckedEffectName) -> Option<EffectId> {
        let effects = self.effects.borrow();
        let id = *effects.ids.get(&name.name)?;
        assert_types(
            name.name.as_str(),
            effects.loaded[id.idx()].types,
            name.types,
        );
        Some(id)
    }
}

impl Effects {
    fn get(&self, name: ErasedEffectName) -> Option<&LoadedEffect> {
        self.ids.get(&name).map(|id| &self.loaded[id.idx()])
    }

    /// Hold `effect` under `name`, at the id the name holds, or at a new one for a new name.
    fn insert(&mut self, name: ErasedEffectName, effect: LoadedEffect) -> EffectId {
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
    use crate::effect::effect_registry::{EffectRegistry, ReducedBy, ReducedName};
    use crate::primitives::effect::effect_kind::EffectKind;
    use crate::primitives::effect::effect_name::EffectName;
    use crate::primitives::effect::effect_params::EffectParams;

    impl EffectRegistry {
        /// The power-saver alternative loaded under `name`; `None` when nothing is loaded.
        pub(crate) fn power_saver_of<U: EffectParams, K: EffectKind>(
            &self,
            name: EffectName<U, K>,
        ) -> Option<ReducedName> {
            let effects = self.effects.borrow();
            effects
                .get(name.erased())
                .map(|loaded| loaded.alternatives[ReducedBy::Power as usize])
        }
    }
}
