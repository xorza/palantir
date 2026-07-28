//! Input-scope resolution: who owns a key this pass.
//!
//! The sub-machine [`crate::input::InputState`] delegates every routing
//! question to, sibling of [`crate::input::watch::Watches`]. It holds one
//! pass's worth of derived state — the path of scopes enclosing the
//! focused widget, the layer that path sits on, and the layer's outermost
//! scope — all rebuilt by [`Scopes::resolve`] at record-pass start.
//!
//! Resolving once per pass rather than per read is what keeps grants
//! independent of where in the pass anything recorded, and it is also
//! what makes the reads cheap: everything that depends only on the
//! cascade is computed here, so [`Scopes::grant`] is a handful of bit
//! tests and [`Scopes::reader`] one containment probe per scope.

use crate::input::key_class::KeyClass;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::{Cascades, ScopeRow};
use crate::scene::layer::Layer;

/// This pass's resolved scope routing.
///
/// **Order is load-bearing.** The cascade appends scopes in pre-order, so
/// an ancestor chain lands outermost-first — which is why [`Self::path`]
/// needs no sort and "innermost" is `rfind` rather than a containment
/// fold. [`Self::resolve`] `debug_assert!`s the property rather than
/// trusting the cascade walk to keep it.
#[derive(Debug, Default)]
pub(super) struct Scopes {
    /// Scopes enclosing the focused widget within [`Self::active_layer`],
    /// **outermost first**. Empty when nothing focused sits in that
    /// layer, which is what [`Self::outermost`] then answers for.
    ///
    /// Capacity is retained across passes, so a stable tree resolves
    /// allocation-free.
    path: Vec<ScopeRow>,
    /// Topmost layer declaring any scope. An overlay declaring one
    /// raises this, which cuts every layer beneath it off both streams —
    /// the whole of what the old keyboard/pointer claim pair did, from
    /// one fact instead of two kept in step.
    active_layer: Option<Layer>,
    /// [`Self::active_layer`]'s outermost scope, resolved once here
    /// because it is a pure function of the cascade and every read would
    /// otherwise recompute it — the one genuinely quadratic step in this
    /// file, paid per pass instead of per chord.
    outermost: Option<WidgetId>,
    /// Scopes withdrawn by [`Self::close`] during the pass being
    /// recorded, honoured by the *next* resolution and then dropped.
    ///
    /// The cascade is one frame stale, so an overlay that decides it is
    /// closing has already recorded its scope and would go on owning
    /// input for the frame after it is gone. This is how it says
    /// otherwise.
    closed: Vec<WidgetId>,
}

impl Scopes {
    /// Rebuild the pass's routing from `focused` and the cascade.
    ///
    /// `focused` is a parameter rather than read back off the input
    /// state because it is the whole input here: a scope path is a
    /// function of where focus sits and what the previous frame
    /// recorded, nothing else.
    pub(super) fn resolve(&mut self, focused: Option<WidgetId>, cascades: &Cascades) {
        self.path.clear();
        let live = |row: &&ScopeRow| !self.closed.contains(&row.id);
        self.active_layer = cascades
            .scopes
            .iter()
            .filter(live)
            .map(|row| row.layer)
            .max();
        if let Some(active) = self.active_layer {
            if let Some(anchor) = focused {
                self.path.extend(
                    cascades
                        .scopes
                        .iter()
                        .filter(live)
                        .filter(|row| row.layer == active && cascades.is_within(anchor, row.id))
                        .copied(),
                );
            }
            self.outermost = outermost_of(active, cascades);
        } else {
            self.outermost = None;
        }
        debug_assert!(
            self.path
                .windows(2)
                .all(|pair| cascades.is_within(pair[1].id, pair[0].id)),
            "scope path must be outermost-first — `grant` reads it as pre-order",
        );
        self.closed.clear();
    }

    /// Withdraw `owner` from the next resolution.
    pub(super) fn close(&mut self, owner: WidgetId) {
        self.closed.push(owner);
    }

    /// Whether an overlay's scope cuts `reader`'s layer off both streams.
    ///
    /// Strictly-below, so the scope's own body keeps reading — a
    /// `TextEdit` inside a `Popup` drains that stream and would otherwise
    /// get nothing.
    pub(super) fn silences(&self, reader: Layer) -> bool {
        self.active_layer
            .is_some_and(|active| active.idx() > reader.idx())
    }

    /// The scope a press of `class` is granted to: the innermost scope on
    /// the path whose filter takes it, else the layer's outermost.
    ///
    /// `rfind`, not a containment fold — the path is outermost-first, so
    /// the last taker *is* the innermost one. Costs one bit test per path
    /// entry and no cascade probe at all.
    pub(super) fn grant(&self, class: KeyClass) -> Option<WidgetId> {
        self.path
            .iter()
            .rfind(|row| row.filter.takes(class))
            .map(|row| row.id)
            .or(self.outermost)
    }

    /// Which scope a read taken at `parent` on `reader` speaks for.
    ///
    /// `None` silences the read outright — a layer strictly below the
    /// active one, which is the old capture gate restated. Outside every
    /// scope it is the layer's outermost, which is how a chord handler
    /// that records nothing (darkroom's navigation phase) still reads as
    /// the application root.
    pub(super) fn reader(
        &self,
        reader: Layer,
        parent: Option<WidgetId>,
        cascades: &Cascades,
    ) -> Option<WidgetId> {
        let active = self.active_layer?;
        if reader.idx() < active.idx() {
            return None;
        }
        let Some(parent) = parent else {
            return self.outermost;
        };
        cascades
            .scopes
            .iter()
            .rfind(|row| row.layer == active && cascades.is_within(parent, row.id))
            .map(|row| row.id)
            .or(self.outermost)
    }
}

/// `active`'s outermost scope: drop every scope contained in another,
/// then take the last of what is left.
///
/// **Outermost, not last-recorded.** Scopes record in pre-order, so the
/// last one is the innermost — taking it would make an app root's
/// accelerators resolve as the focused text field nested inside it, and
/// every one of them would die mid-edit. Record order then breaks the tie
/// between what survives, which is two *sibling* overlays on one layer,
/// exactly as the old topmost-claim did.
///
/// Quadratic in the layer's scope count, which is why [`Scopes::resolve`]
/// calls it once and caches the answer.
fn outermost_of(active: Layer, cascades: &Cascades) -> Option<WidgetId> {
    let in_layer = || cascades.scopes.iter().filter(|row| row.layer == active);
    in_layer()
        .rev()
        .find(|row| {
            !in_layer().any(|other| other.id != row.id && cascades.is_within(row.id, other.id))
        })
        .map(|row| row.id)
}
