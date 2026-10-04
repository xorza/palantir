//! State a kind of widget shares across all its instances: one value per
//! type, for the life of the `Ui`.

use crate::common::typed_stores::DOWNCAST_ERROR;
use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::fmt;

/// One boxed value per type, never swept — the home of state no widget
/// id owns, such as the clock that lets one tooltip after another show
/// at once. Keyed by type, so a widget's private type is a key no other
/// code can collide with.
#[derive(Default)]
pub(crate) struct Singletons {
    by_type: FxHashMap<TypeId, Box<dyn Any>>,
}

// Manual: the values are `dyn Any`, which has no `Debug`. The count is
// the shape worth reporting.
impl fmt::Debug for Singletons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Singletons")
            .field("values", &self.by_type.len())
            .finish_non_exhaustive()
    }
}

impl Singletons {
    pub(super) fn get<S: 'static>(&self) -> Option<&S> {
        self.by_type
            .get(&TypeId::of::<S>())
            .map(|value| value.downcast_ref::<S>().expect(DOWNCAST_ERROR))
    }

    pub(super) fn get_mut<S: 'static>(&mut self) -> Option<&mut S> {
        self.by_type
            .get_mut(&TypeId::of::<S>())
            .map(|value| value.downcast_mut::<S>().expect(DOWNCAST_ERROR))
    }

    pub(super) fn get_or_default<S: Default + 'static>(&mut self) -> &mut S {
        self.by_type
            .entry(TypeId::of::<S>())
            .or_insert_with(|| Box::<S>::default())
            .downcast_mut::<S>()
            .expect(DOWNCAST_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::singletons::Singletons;

    #[derive(Debug, Default, PartialEq)]
    struct Counter(u32);

    #[derive(Debug, Default, PartialEq)]
    struct Other(u32);

    /// Absent until first use, defaulted then, kept after, and one per
    /// type — two types never share a value.
    #[test]
    fn one_value_per_type_kept_across_reads() {
        let mut singletons = Singletons::default();
        assert_eq!(singletons.get::<Counter>(), None);
        assert_eq!(
            singletons.get_mut::<Counter>(),
            None,
            "a probe stores nothing"
        );
        assert_eq!(singletons.get::<Counter>(), None);
        singletons.get_or_default::<Counter>().0 = 3;
        singletons.get_or_default::<Other>().0 = 7;
        assert_eq!(singletons.get::<Counter>(), Some(&Counter(3)));
        assert_eq!(singletons.get::<Other>(), Some(&Other(7)));
        singletons.get_or_default::<Counter>().0 += 1;
        assert_eq!(singletons.get::<Counter>(), Some(&Counter(4)));
    }
}
