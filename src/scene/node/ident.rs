//! How a widget's `WidgetId` is derived and the id it resolved to.

use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::seen_ids::ResolvedId;
use std::panic::Location;

/// A [`Widget`](crate::widget_core::widget::Widget)'s identity: a recipe until
/// first contact with `Ui`, then the id it resolved to.
///
/// - [`Ident::Auto`]: the `#[track_caller]` call site, hashed only when
///   `SeenIds` cannot match it to last frame's. Parent-scoped (mixed with the
///   open parent's resolved id like [`Ident::Hash`]), since a loop or helper
///   yields one base id; sibling collisions get an occurrence counter.
/// - [`Ident::Hash`]: a raw `.id_salt(key)` hash mixed with the open parent's
///   resolved id (`Layer::Main`'s synthetic viewport counts), so state survives subtree moves.
/// - [`Ident::Verbatim`]: a precomputed id from `.id(id)`, used as-is; the only
///   recipe that skips parent-scoping.
/// - [`Ident::Resolved`]: what the widget records under, stored because
///   resolution is not pure (`SeenIds` bumps a raw id another widget opened).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Ident {
    Auto(&'static Location<'static>),
    Hash(WidgetId),
    Verbatim(WidgetId),
    Resolved(ResolvedId),
}

impl Ident {
    /// Mixes `self` with `parent`'s resolved id into the raw id `SeenIds`
    /// disambiguates. `Resolved` never reaches here, or it would be
    /// disambiguated twice. `parent == None` is the root of a side layer.
    #[inline]
    pub(crate) fn raw_id(self, parent: Option<WidgetId>) -> WidgetId {
        match self {
            Ident::Verbatim(id) => id,
            Ident::Resolved(resolved) => {
                unreachable!("resolved id {:?} fed back to the forest", resolved.id())
            }
            Ident::Auto(site) => WidgetId::from_location(site).scoped(parent),
            Ident::Hash(id) => id.scoped(parent),
        }
    }

    /// `true` for every identity but [`Ident::Auto`]; `SeenIds::resolve` flags explicit collisions as caller bugs.
    #[inline]
    pub(crate) const fn is_explicit(self) -> bool {
        !matches!(self, Ident::Auto(_))
    }
}
