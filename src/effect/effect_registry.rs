//! [`EffectRegistry`]: the effects a host loaded, by name, shared by every window.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::effect::effect_source::{EffectSource, ReducedEffect};
use crate::effect::effect_store::{EffectModule, EffectStore};
use crate::effect::error::EffectError;
use crate::effect::stock::{self, StockEffect};
use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::effect::effect_kind::{EffectKind, EffectTypes};
use crate::primitives::effect::effect_name::{CheckedEffectName, EffectName, ErasedEffectName};
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::backdrop_reach::BackdropReach;
use crate::primitives::paint::material::material_effect::MaterialEffect;
use crate::primitives::reduction::Reductions;

/// Every effect loaded under a name, for the app's life, as a font is: loading the same name again
/// replaces it, and a refused load leaves the one before. A name keeps the parameter type and
/// kind it first loaded with; using it with others is a logic error that panics with both. Each
/// load goes to the [`EffectStore`] the host built the registry over, which compiles it; a
/// deviceless recorder's registry has none. It holds palantir's stock effects
/// (`crate::effect::stock`) from its start, checked as an app's load is, once a process, under names
/// no app load takes.
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
    /// How many lanes a material over it holds; `None` for an effect no material wears.
    material_lanes: Option<u8>,
    /// How far past its shape a backdrop effect reads; `None` for a color effect.
    reach: Option<ErasedReach>,
}

/// How many lanes a [`Material`] over an effect of `source` holds; `None` unless it is a backdrop
/// effect that binds no image, over parameters a [`MaterialEffect`] holds
/// ([`MaterialEffect::lanes_for`]), as a box hands its material no image.
fn source_material_lanes<U: EffectParams, K: EffectKind>(
    source: &EffectSource<U, K>,
) -> Option<u8> {
    if K::ID != KindId::Backdrop || source.image_slots > 0 {
        return None;
    }
    let lanes = MaterialEffect::lanes_for::<U>()?;
    Some(u8::try_from(lanes).expect("at most 8 lanes"))
}

/// A backdrop effect's [`EffectSource::reach`], its parameters' type erased, so a material and an
/// `EffectPaint` over the effect read it alike from their parameter bytes.
struct ErasedReach(Box<ReachOfBytes>);

/// A reach over parameter bytes.
type ReachOfBytes = dyn Fn(&[u8]) -> BackdropReach;

impl ErasedReach {
    /// `source`'s reach; `None` for a color effect, which reads nothing under it.
    fn of<U: EffectParams, K: EffectKind>(source: &EffectSource<U, K>) -> Option<Self> {
        if K::ID != KindId::Backdrop {
            return None;
        }
        let reach = source.reach;
        Some(Self(Box::new(move |params: &[u8]| {
            reach(&bytemuck::pod_read_unaligned::<U>(params))
        })))
    }
}

impl fmt::Debug for ErasedReach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ErasedReach")
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
    pub(crate) reach: BackdropReach,
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

/// [`ReducedEffect`] resolved at the load: the alternative is loaded by then, with the types and
/// images it was checked against, so it is held by the id its name holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReducedName {
    Same,
    Other(EffectId),
    Nothing,
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
        let stock = stock::effects();
        for effect in &stock.press {
            registry.insert_stock(effect);
        }
        registry.insert_stock(&stock.frosted);
        registry.insert_stock(&stock.glass);
        registry
    }

    /// Load `source` under `name`, replacing the effect loaded under it before.
    ///
    /// # Panics
    ///
    /// When `name` was loaded before with another parameter type or kind; when an alternative its
    /// source names is not loaded, or with other types or image slots; when a reload of `name`
    /// changes the image slots of an alternative another effect paints in its place.
    pub(crate) fn load<U: EffectParams, K: EffectKind>(
        &self,
        name: EffectName<U, K>,
        source: &EffectSource<U, K>,
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
        source: &EffectSource<U, K>,
    ) -> Result<(), EffectError> {
        self.hold_types(name, source);
        let module = source.check()?;
        self.record(name, source, &module);
        Ok(())
    }

    /// Load a stock effect, its module checked once a process ([`stock::effects`]).
    fn insert_stock<U: EffectParams, K: EffectKind>(&self, effect: &StockEffect<U, K>) {
        let StockEffect {
            name,
            source,
            module,
        } = effect;
        self.hold_types(*name, source);
        self.record(*name, source, module);
    }

    /// Panic unless `name` takes the types it was loaded with, each alternative `source` names is
    /// loaded with the same types and images, as its shapes pass both, and a reload of `name`
    /// keeps the images of each effect it paints in place of.
    ///
    /// The alternative is held at the load, not where it paints: a missing one would panic only
    /// on a machine whose person reduces motion or power, which a developer may never run.
    /// Refused: an alternative loaded later, checked as it paints, as before.
    #[track_caller]
    fn hold_types<U: EffectParams, K: EffectKind>(
        &self,
        name: EffectName<U, K>,
        source: &EffectSource<U, K>,
    ) {
        let types = EffectTypes::of::<U, K>();
        let effects = self.effects.borrow();
        if let Some(&id) = effects.ids.get(&name.erased()) {
            assert_types(name.name(), effects.loaded[id.idx()].types, types);
            // An effect that paints this one in its place bound the images it has now.
            // An effect that names itself is checked below, by the source that replaces it.
            for (namer, loaded) in effects.ids.iter().filter_map(|(namer, other)| {
                let loaded = &effects.loaded[other.idx()];
                (*other != id && loaded.alternatives.contains(&ReducedName::Other(id)))
                    .then_some((namer, loaded))
            }) {
                assert!(
                    loaded.image_slots == source.image_slots,
                    "effect `{}` binds {} images, and `{}`, which paints in its place, {}",
                    namer.as_str(),
                    loaded.image_slots,
                    name.name(),
                    source.image_slots,
                );
            }
        }
        for (by, paint) in ReducedBy::ALL.into_iter().zip(source.alternatives()) {
            // An effect that names itself paints the source loading now, which holds itself.
            let ReducedEffect::Other(other) = paint else {
                continue;
            };
            if other.erased() == name.erased() {
                continue;
            }
            let loaded = effects.get(other.erased()).unwrap_or_else(|| {
                panic!(
                    "no effect is loaded under `{}`, the {} alternative of `{}`: load it first",
                    other.name(),
                    by.name(),
                    name.name(),
                )
            });
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

    /// Hold `source`, which `module` checked, under `name`, at the id the name holds, and hand
    /// the module to the store.
    fn record<U: EffectParams, K: EffectKind>(
        &self,
        name: EffectName<U, K>,
        source: &EffectSource<U, K>,
        module: &EffectModule,
    ) {
        let id = self
            .effects
            .borrow_mut()
            .insert(name.erased(), |ids| LoadedEffect {
                types: EffectTypes::of::<U, K>(),
                image_slots: source.image_slots,
                alternatives: source.alternatives().map(|paint| match paint {
                    ReducedEffect::Same => ReducedName::Same,
                    ReducedEffect::Other(other) => ReducedName::Other(
                        *ids.get(&other.erased())
                            .expect("`hold_types` found the alternative loaded, or it is this one"),
                    ),
                    ReducedEffect::Nothing => ReducedName::Nothing,
                }),
                material_lanes: source_material_lanes(source),
                reach: ErasedReach::of(source),
            });
        if let Some(store) = &self.store {
            store.register(id, module);
            if source.prebuild {
                store.want(id);
            }
        }
        self.epoch.set(self.epoch.get().wrapping_add(1));
    }

    /// The types the effect under `name` loaded with; `None` when nothing is loaded under it. A
    /// check of a name that untrusted data holds, as a theme file's, which may name anything.
    pub(crate) fn types_of(&self, name: ErasedEffectName) -> Option<EffectTypes> {
        self.effects.borrow().get(name).map(|loaded| loaded.types)
    }

    /// Want the effect loaded under `name` built before the next frame draws, with each
    /// alternative its source names, as a theme that names it does: no frame after builds them
    /// again, whatever it reduces.
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

    /// Want `id` built, with each alternative its source names.
    fn want_id(&self, id: EffectId) {
        let Some(store) = &self.store else {
            return;
        };
        store.want(id);
        let effects = self.effects.borrow();
        for alternative in effects.loaded[id.idx()].alternatives {
            if let ReducedName::Other(other) = alternative {
                store.want(other);
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
    /// When no effect is loaded under `name`, or one with another parameter type or kind: a shape
    /// that names an effect nothing loaded is a logic error, as painting nothing would be quietly
    /// wrong.
    #[track_caller]
    pub(crate) fn resolve(
        &self,
        name: CheckedEffectName,
        reductions: Reductions,
    ) -> Option<ResolvedEffect> {
        let id = self
            .id_of(name)
            .unwrap_or_else(|| panic!("no effect is loaded under `{}`", name.name.as_str()));
        let painted = self.painted(id, reductions)?;
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
            Some(LoadedEffect {
                material_lanes: None,
                ..
            }) => MaterialFit::NotMaterial,
            Some(LoadedEffect {
                material_lanes: Some(lanes),
                ..
            }) => MaterialFit::Lanes(*lanes),
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
    /// When nothing is loaded under the material's effect, or the effect is no material's, or
    /// takes another count of lanes than the material holds: a material that cannot paint is a
    /// logic error, as painting nothing would be quietly wrong. An alternative it paints in place
    /// of shares its types and images, so it is a material's too.
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
            let want = Self::material_lanes(&effects, id, name);
            let got = material.effect.count();
            assert!(
                want == got,
                "effect `{}` takes {want} parameter lanes, and its material holds {got}",
                name.as_str()
            );
            id
        };
        let painted = self.painted(id, reductions)?;
        Some(ResolvedMaterial {
            id: painted,
            reach: self.reach_of(painted, material.effect.bytes()),
        })
    }

    /// How far past its shape the backdrop effect at `id` reads over `params`, its parameter
    /// bytes, as its source declares ([`EffectSource::reach`]), for a material and an
    /// `EffectPaint` alike.
    ///
    /// # Panics
    ///
    /// When the effect is a color effect, which reads nothing under it: a paint's kind is its
    /// name's, so only a logic error asks.
    pub(crate) fn reach_of(&self, id: EffectId, params: &[u8]) -> BackdropReach {
        let effects = self.effects.borrow();
        let reach = effects.loaded[id.idx()]
            .reach
            .as_ref()
            .expect("a backdrop effect keeps its reach");
        (reach.0)(params)
    }

    /// How many lanes a material over the effect at `id` holds, the effect a material over
    /// `name` paints.
    ///
    /// # Panics
    ///
    /// When the effect is no material's.
    #[track_caller]
    fn material_lanes(effects: &Effects, id: EffectId, name: ErasedEffectName) -> u8 {
        effects.loaded[id.idx()].material_lanes.unwrap_or_else(|| {
            panic!(
                "a material over `{}` paints effect {}, which is no material's: a backdrop effect \
                 over at most {} f32 lanes that binds no image",
                name.as_str(),
                id.0,
                MaterialEffect::MAX_LANES
            )
        })
    }

    /// The effect a shape that names `id` paints: `id`, or the alternative its source names for
    /// the first preference `reductions` reduces that names one, in [`ReducedBy`]'s order; `None`
    /// when that is nothing. The load held the alternative to the effect's types and images.
    fn painted(&self, id: EffectId, reductions: Reductions) -> Option<EffectId> {
        let effects = self.effects.borrow();
        let alternative = ReducedBy::ALL
            .into_iter()
            .zip(effects.loaded[id.idx()].alternatives)
            .find(|&(by, alternative)| {
                alternative != ReducedName::Same && by.is_reduced_in(reductions)
            })
            .map_or(ReducedName::Same, |(_, alternative)| alternative);
        match alternative {
            ReducedName::Same => Some(id),
            ReducedName::Nothing => None,
            ReducedName::Other(other) => Some(other),
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

    /// Hold the effect `effect` builds under `name`, at the id the name holds, or at a new one for
    /// a new name; `effect` reads the ids with `name`'s already in, so an effect can name itself.
    fn insert(
        &mut self,
        name: ErasedEffectName,
        effect: impl FnOnce(&FxHashMap<ErasedEffectName, EffectId>) -> LoadedEffect,
    ) -> EffectId {
        let next = EffectId(u32::try_from(self.loaded.len()).expect("fewer than 2³² effects"));
        let id = *self.ids.entry(name).or_insert(next);
        let effect = effect(&self.ids);
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
    use crate::primitives::identity::effect_id::EffectId;

    impl EffectRegistry {
        /// The id `name` holds.
        pub(crate) fn id_of_name<U: EffectParams, K: EffectKind>(
            &self,
            name: EffectName<U, K>,
        ) -> EffectId {
            self.id_of(name.checked()).expect("the name is loaded")
        }

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
