//! How a node's `WidgetId` is derived: an explicit id, a caller-supplied
//! salt, or the call site itself.

use crate::primitives::widget_id::WidgetId;

/// Recipe for a [`Node`](crate::scene::node::Node)'s `WidgetId`. Mirrors egui's
/// `Option<Id>` "raw `id_salt`, resolve at `Ui::widget`"
/// pattern: the builder stores the user's intent, resolution happens
/// at record time when the parent context is known. Three sources:
///
/// - [`Salt::Auto`] — `#[track_caller]`-derived. The captured
///   `(file, line, column)` encodes call-site identity, but a call
///   site reached from a loop or helper resolves to the *same* base id
///   for every iteration, so identity must also depend on **where in
///   the tree** the widget sits. So an auto id is **parent-scoped**
///   too: mixed with the most-recently-opened parent's resolved
///   `WidgetId`, exactly like [`Salt::Hash`]. This is what keeps two
///   nodes drawn from one `draw_one` helper — whose interior text /
///   shape leaves share an auto call site — from swapping ids when the
///   nodes' paint order flips: each leaf hangs off its own stable-id
///   node body, so a raise/reorder can't churn its identity (and thus
///   can't spuriously damage or re-key state for untouched nodes).
///   Same-parent collisions from a genuine sibling loop are still
///   disambiguated by `SeenIds`' occurrence counter.
///
/// - [`Salt::Hash`] — raw user-supplied hash from `.id_salt(key)`.
///   At resolve time the hash is **mixed with the most-recently-
///   opened parent's resolved `WidgetId`** in the current layer
///   (`Layer::Main`'s synthetic viewport counts as a parent — its
///   `Salt::Auto` id is stable across frames). Two `.id_salt("row")`
///   under different parents resolve to distinct ids, so per-widget
///   `StateMap` / focus / animation entries survive subtree moves
///   without manual `WidgetId::with` chaining. Matches egui.
///
/// - [`Salt::Verbatim`] — precomputed [`WidgetId`] from `.id(id)`,
///   used as-is. Escape hatch for ids derived elsewhere
///   (cross-layer popups, sibling pairs sharing a seed). The **only**
///   variant that skips parent-scoping.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Salt {
    Auto(WidgetId),
    Hash(WidgetId),
    Verbatim(WidgetId),
}

impl Salt {
    /// Mix `self` with `parent`'s already-resolved `WidgetId` to
    /// produce the id that will be recorded into the tree.
    /// [`Salt::Auto`] and [`Salt::Hash`] both consult `parent` (so a
    /// widget's identity tracks its position in the tree, not its
    /// global record order); only [`Salt::Verbatim`] passes through.
    /// `parent == None` covers the "no open node at all" case (the
    /// root of a side layer). `Layer::Main`'s synthetic viewport
    /// counts as a parent with a frame-stable id, so top-level widgets
    /// resolve to `VIEWPORT.with(salt)` like any other parent-scoped
    /// id.
    #[inline]
    pub(crate) fn resolve(self, parent: Option<WidgetId>) -> WidgetId {
        match self {
            Salt::Verbatim(id) => id,
            Salt::Auto(id) | Salt::Hash(id) => match parent {
                Some(p) => p.with(id.0),
                None => id,
            },
        }
    }

    /// `true` for [`Salt::Hash`] / [`Salt::Verbatim`] — caller-supplied
    /// ids. `SeenIds::resolve` uses this to flag explicit collisions
    /// (caller bugs) with the magenta debug overlay while leaving
    /// auto collisions silent.
    #[inline]
    pub(crate) fn is_explicit(self) -> bool {
        matches!(self, Salt::Hash(_) | Salt::Verbatim(_))
    }
}
