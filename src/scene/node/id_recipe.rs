//! How a widget's `WidgetId` is derived: the recipe the forest resolves into the id a widget
//! records under.

use crate::primitives::identity::widget_id::WidgetId;
use std::panic::Location;

/// What a widget's id is derived from, which [`Forest::widget_id`](crate::scene::forest::Forest::widget_id)
/// resolves against the open parent. The resolved id is the widget's to keep, never a recipe, so
/// no resolved id reaches the forest twice. Refused: one enum of the recipes and the result,
/// whose result arm the forest could only refuse with a panic.
#[derive(Clone, Copy, Debug)]
pub(crate) enum IdRecipe {
    /// Mixed with the open parent's resolved id ([`ScopedRecipe`]).
    Scoped(ScopedRecipe),
    /// A precomputed id from `.id(id)`, used as-is; the only recipe that skips parent-scoping.
    Verbatim(WidgetId),
}

/// A recipe mixed with the open parent's resolved id (`Layer::Main`'s synthetic viewport counts),
/// so state survives subtree moves. `SeenIds` matches one to last frame's by its inputs, so an
/// unchanged frame never hashes the raw id.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ScopedRecipe {
    /// The `#[track_caller]` call site, hashed only when `SeenIds` cannot match it to last
    /// frame's; a loop or helper yields one base id, and sibling collisions get an occurrence
    /// counter.
    Auto(&'static Location<'static>),
    /// A raw `.id_salt(key)` hash.
    Hash(WidgetId),
}

impl ScopedRecipe {
    #[inline]
    pub(crate) const fn kind(self) -> IdKind {
        match self {
            Self::Auto(_) => IdKind::Auto,
            Self::Hash(_) => IdKind::Explicit,
        }
    }

    /// `self` mixed with `parent`'s resolved id.
    #[inline]
    pub(crate) fn raw_id(self, parent: Option<WidgetId>) -> WidgetId {
        match self {
            Self::Auto(site) => WidgetId::from_location(site).scoped(parent),
            Self::Hash(id) => id.scoped(parent),
        }
    }
}

/// Where a raw id came from, which decides what its collision means: two call sites
/// sharing one id is a loop or helper and is disambiguated quietly, two ids the caller
/// named alike is a caller bug `SeenIds::resolve` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IdKind {
    /// A `#[track_caller]` call site.
    Auto,
    /// Named by `.id(..)` or `.id_salt(..)`.
    Explicit,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::scene::node::id_recipe::IdRecipe;

    impl IdRecipe {
        /// The raw id `SeenIds` disambiguates: `self` mixed with `parent`'s resolved id, or the
        /// verbatim id as it is.
        pub(crate) fn raw_id(self, parent: Option<WidgetId>) -> WidgetId {
            match self {
                Self::Scoped(scoped) => scoped.raw_id(parent),
                Self::Verbatim(id) => id,
            }
        }
    }
}
