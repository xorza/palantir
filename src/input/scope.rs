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
    /// otherwise recompute it — one scan of the layer's scopes per pass
    /// instead of per chord.
    outermost: Option<WidgetId>,
    /// Scopes withdrawn by [`Self::close`] during the frame being
    /// recorded.
    ///
    /// The cascade is one frame stale, so an overlay that decides it is
    /// closing has already recorded its scope and would go on owning
    /// input for the frame after it is gone. This is how it says
    /// otherwise.
    closing: Vec<WidgetId>,
    /// …and the frame before, still honoured because the cascade this
    /// pass resolves against is the one that frame left behind.
    ///
    /// **A close has to outlive its own pass, and exactly one frame
    /// boundary.** A dismissal is action input, so its frame always
    /// records twice; clearing per resolve let pass B wipe pass A's
    /// close, and pass B cannot re-issue it because the dismissing edge
    /// was drained between them. Holding it a frame longer would instead
    /// suppress a popup the host reopens under the same id on the very
    /// next frame — what right-clicking through an open context menu
    /// does. [`Self::end_frame`] swaps, which makes that lifetime
    /// structural rather than arithmetic.
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
        let live = || live_scopes(cascades, &self.closing, &self.closed);
        self.active_layer = live().map(|row| row.layer).max();
        if let Some(active) = self.active_layer {
            if let Some(anchor) = focused {
                self.path.extend(
                    live().filter(|row| row.layer == active && cascades.is_within(anchor, row.id)),
                );
            }
            // **Outermost, not last-recorded.** Taking the last row would
            // make an app root's accelerators resolve as the focused text
            // field nested inside it, and every one of them would die
            // mid-edit. Scopes record in pre-order, so each one's subtree
            // is a contiguous run: a row inside the standing root is a
            // descendant and skipped, and the first row outside it opens
            // the next root. The fold therefore ends on the last root,
            // which is what breaks the tie between two *sibling* overlays
            // on one layer — exactly as the old topmost-claim did.
            self.outermost = live()
                .filter(|row| row.layer == active)
                .fold(None, |root, row| match root {
                    Some(id) if cascades.is_within(row.id, id) => Some(id),
                    _ => Some(row.id),
                });
        } else {
            self.outermost = None;
        }
        debug_assert!(
            self.path
                .windows(2)
                .all(|pair| cascades.is_within(pair[1].id, pair[0].id)),
            "scope path must be outermost-first — `grant` reads it as pre-order",
        );
    }

    /// Withdraw `owner` for the rest of this frame and all of the next —
    /// see [`Self::closed`] for why that span.
    pub(super) fn close(&mut self, owner: WidgetId) {
        if !self.closing.contains(&owner) {
            self.closing.push(owner);
        }
    }

    /// Age this frame's withdrawals into the next one. Called once per
    /// frame, after the last record pass.
    pub(super) fn end_frame(&mut self) {
        self.closed.clear();
        std::mem::swap(&mut self.closed, &mut self.closing);
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

    /// Which scope a read taken at `parent` speaks for.
    ///
    /// Outside every scope it is the layer's outermost, which is how a
    /// chord handler that records nothing (darkroom's navigation phase)
    /// still reads as the application root.
    ///
    /// `None` means **no scope exists at all**, not "silenced" — an app
    /// that declares none must keep every chord working, so this and
    /// [`Self::grant`] both answer `None` there and compare equal.
    /// Silencing is [`Self::silences`]'s job, and the caller checks it
    /// first.
    pub(super) fn reader(&self, parent: Option<WidgetId>, cascades: &Cascades) -> Option<WidgetId> {
        let active = self.active_layer?;
        let Some(parent) = parent else {
            return self.outermost;
        };
        live_scopes(cascades, &self.closing, &self.closed)
            .rfind(|row| row.layer == active && cascades.is_within(parent, row.id))
            .map(|row| row.id)
            .or(self.outermost)
    }
}

/// The scope rows still owning input: `cascades`'s, minus everything
/// [`Scopes::close`] withdrew.
///
/// **Every scan over `cascades.scopes` goes through here.** The cascade
/// is a frame stale, so it still lists closed overlays; a scan that reads
/// it raw resolves grants onto a scope that is gone and strands the
/// surviving one — silently, since the raw stream and the layer gate both
/// keep working.
///
/// Takes the two withdrawal columns rather than `&Scopes`, because
/// [`Scopes::resolve`] scans while filling `path` and only a field-level
/// borrow leaves that field free.
///
/// `copied` before `filter`, so the predicate takes `&ScopeRow` rather
/// than the `&&ScopeRow` a borrowing iterator would hand it.
fn live_scopes<'a>(
    cascades: &'a Cascades,
    closing: &'a [WidgetId],
    closed: &'a [WidgetId],
) -> impl DoubleEndedIterator<Item = ScopeRow> + 'a {
    cascades
        .scopes
        .iter()
        .copied()
        .filter(move |row| !closing.contains(&row.id) && !closed.contains(&row.id))
}
