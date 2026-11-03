//! State a widget kind shares across instances: one value per type for the `Ui`'s life.

use crate::primitives::identity::typed_stores::DOWNCAST_ERROR;
use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::fmt;
use std::mem;

/// What a read or a lend of a lent singleton panics with.
pub(crate) const LENT_SINGLETON: &str =
    "this singleton is lent to a `Ui::with_singleton` body: read it through the body's `&mut S`";

/// One boxed value per type, never swept: state no widget id owns, such as the clock that lets one tooltip after another show at once.
#[derive(Default)]
pub(crate) struct Singletons {
    by_type: FxHashMap<TypeId, Slot>,
}

/// A type's value, or the mark that it is lent out, so a read or a second lend inside the lend
/// panics rather than finds a default.
enum Slot {
    Held(Box<dyn Any>),
    Lent,
}

// Manual: the values are `dyn Any`, which has no `Debug`; the count is what's worth reporting.
impl fmt::Debug for Singletons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Singletons")
            .field("values", &self.by_type.len())
            .finish_non_exhaustive()
    }
}

impl Singletons {
    /// # Panics
    ///
    /// While `S` is lent out.
    #[track_caller]
    pub(super) fn get<S: 'static>(&self) -> Option<&S> {
        match self.by_type.get(&TypeId::of::<S>())? {
            Slot::Held(value) => Some(value.downcast_ref::<S>().expect(DOWNCAST_ERROR)),
            Slot::Lent => panic!("{LENT_SINGLETON}"),
        }
    }

    /// Lend the `S` out, `S::default()` on first use; [`Self::put`] returns it. The box goes
    /// out and back, so a lend builds no value.
    ///
    /// # Panics
    ///
    /// While `S` is lent out already.
    #[track_caller]
    pub(super) fn take<S: Default + 'static>(&mut self) -> Box<S> {
        let slot = self
            .by_type
            .entry(TypeId::of::<S>())
            .or_insert_with(|| Slot::Held(Box::<S>::default()));
        match mem::replace(slot, Slot::Lent) {
            Slot::Held(value) => value.downcast::<S>().expect(DOWNCAST_ERROR),
            Slot::Lent => panic!("{LENT_SINGLETON}"),
        }
    }

    /// Return the `S` [`Self::take`] lent out.
    pub(super) fn put<S: 'static>(&mut self, value: Box<S>) {
        let slot = self
            .by_type
            .get_mut(&TypeId::of::<S>())
            .expect("a singleton returns to the slot it was lent from");
        debug_assert!(matches!(slot, Slot::Lent), "a singleton returned twice");
        *slot = Slot::Held(value);
    }
}

#[cfg(test)]
mod tests {
    use crate::internals::panic_probe;
    use crate::ui::singletons::{LENT_SINGLETON, Singletons};

    #[derive(Debug, Default, PartialEq)]
    struct Counter(u32);

    #[derive(Debug, Default, PartialEq)]
    struct Other(u32);

    fn add(singletons: &mut Singletons, n: u32) {
        let mut value = singletons.take::<Counter>();
        value.0 += n;
        singletons.put(value);
    }

    /// Absent until first use, defaulted then, kept after, one per type.
    #[test]
    fn one_value_per_type_kept_across_reads() {
        let mut singletons = Singletons::default();
        assert_eq!(singletons.get::<Counter>(), None, "a probe stores nothing");
        assert_eq!(singletons.get::<Counter>(), None);
        add(&mut singletons, 3);
        let mut other = singletons.take::<Other>();
        other.0 = 7;
        singletons.put(other);
        assert_eq!(singletons.get::<Counter>(), Some(&Counter(3)));
        assert_eq!(singletons.get::<Other>(), Some(&Other(7)));
        add(&mut singletons, 1);
        assert_eq!(singletons.get::<Counter>(), Some(&Counter(4)));
    }

    /// While `Counter` is lent, a read of it and a second lend panic; `Other` stays readable.
    #[test]
    fn a_lent_value_refuses_a_read_and_a_second_lend() {
        let mut singletons = Singletons::default();
        add(&mut singletons, 1);
        let _lent = singletons.take::<Counter>();
        assert_eq!(singletons.get::<Other>(), None);
        panic_probe::assert_panics_with(LENT_SINGLETON, || {
            singletons.get::<Counter>();
        });
        panic_probe::assert_panics_with(LENT_SINGLETON, || {
            Singletons::take::<Counter>(&mut singletons);
        });
    }
}
