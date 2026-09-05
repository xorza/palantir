//! How a widget's `WidgetId` is derived — an explicit id, a caller-supplied
//! salt, or the call site itself — and the id it became once derived.

use crate::primitives::widget_id::WidgetId;

/// A [`Widget`](crate::widgets::widget::Widget)'s identity, in one of two
/// halves of its life. Before the widget's first contact with `Ui` it is a
/// *recipe*, mirroring egui's `Option<Id>` "raw `id_salt`, resolve at
/// record" pattern: the builder stores the user's intent, and resolution
/// happens when the parent context is known. After, it is the id that
/// recipe resolved to, and every later read and the record use that one.
///
/// Three recipes:
///
/// - [`Ident::Auto`] — `#[track_caller]`-derived. The captured
///   `(file, line, column)` encodes call-site identity, but a call
///   site reached from a loop or helper resolves to the *same* base id
///   for every iteration, so identity must also depend on **where in
///   the tree** the widget sits. So an auto id is **parent-scoped**
///   too: mixed with the most-recently-opened parent's resolved
///   `WidgetId`, exactly like [`Ident::Hash`]. This is what keeps two
///   nodes drawn from one `draw_one` helper — whose interior text /
///   shape leaves share an auto call site — from swapping ids when the
///   nodes' paint order flips: each leaf hangs off its own stable-id
///   node body, so a raise/reorder can't churn its identity (and thus
///   can't spuriously damage or re-key state for untouched nodes).
///   Same-parent collisions from a genuine sibling loop are still
///   disambiguated by `SeenIds`' occurrence counter.
///
/// - [`Ident::Hash`] — raw user-supplied hash from `.id_salt(key)`.
///   At resolve time the hash is **mixed with the most-recently-
///   opened parent's resolved `WidgetId`** in the current layer
///   (`Layer::Main`'s synthetic viewport counts as a parent — its
///   [`Ident::Auto`] id is stable across frames). Two `.id_salt("row")`
///   under different parents resolve to distinct ids, so per-widget
///   `StateMap` / focus / animation entries survive subtree moves
///   without manual `WidgetId::with` chaining. Matches egui.
///
/// - [`Ident::Verbatim`] — precomputed [`WidgetId`] from `.id(id)`,
///   used as-is. Escape hatch for ids derived elsewhere
///   (cross-layer popups, sibling pairs sharing a seed). The **only**
///   recipe that skips parent-scoping.
///
/// And the outcome, [`Ident::Resolved`]: what the widget records under.
/// Stored on the widget rather than re-derived at record, because the
/// derivation is not a pure function — `SeenIds` bumps a raw id that
/// another widget opened in between, and a read made under the first
/// answer must not be recorded under the second.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Ident {
    Auto(WidgetId),
    Hash(WidgetId),
    Verbatim(WidgetId),
    Resolved(WidgetId),
}

impl Ident {
    /// Mix `self` with `parent`'s already-resolved `WidgetId` to
    /// produce the raw id `SeenIds` disambiguates into the recorded one.
    /// [`Ident::Auto`] and [`Ident::Hash`] both consult `parent` (so a
    /// widget's identity tracks its position in the tree, not its
    /// global record order); [`Ident::Verbatim`] passes through, and so
    /// does [`Ident::Resolved`], which is past this step.
    /// `parent == None` covers the "no open node at all" case (the
    /// root of a side layer). `Layer::Main`'s synthetic viewport
    /// counts as a parent with a frame-stable id, so top-level widgets
    /// resolve to `VIEWPORT.with(salt)` like any other parent-scoped
    /// id.
    #[inline]
    pub(crate) fn raw_id(self, parent: Option<WidgetId>) -> WidgetId {
        match self {
            Ident::Verbatim(id) | Ident::Resolved(id) => id,
            Ident::Auto(id) | Ident::Hash(id) => match parent {
                Some(p) => p.with(id.0),
                None => id,
            },
        }
    }

    /// `true` for every identity but [`Ident::Auto`] — the ones the
    /// caller chose, or that already resolved. `SeenIds::resolve` uses
    /// this to flag explicit collisions (caller bugs) with the magenta
    /// debug overlay while leaving auto collisions silent, and the
    /// theme's `default_id` uses it to know whether the caller spoke.
    #[inline]
    pub(crate) fn is_explicit(self) -> bool {
        !matches!(self, Ident::Auto(_))
    }
}
