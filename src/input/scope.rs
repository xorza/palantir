//! Input-scope resolution: who owns a key this pass. Derived state is rebuilt once per pass by [`Scopes::resolve`], so grants don't depend on record position and [`Scopes::grant`] and [`Scopes::reader`] are cheap ([`ReaderMemo`] spares repeat scans).

use crate::cascade::Cascade;
use crate::cascade::cascade_key::CascadeKey;
use crate::cascade::entry::ScopeRow;
use crate::common::counters::TestOnly;
use crate::input::key_class::KeyClass;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use std::mem;

/// This pass's resolved scope routing.
///
/// **Order is load-bearing.** The cascade appends scopes in pre-order, so ancestor chains land outermost-first: [`Self::path`] needs no sort and "innermost" is `rfind`. [`Self::resolve`] debug-asserts it.
///
/// # Mid-pass changes wait for the next resolution
///
/// A pass routes by the state it started with. A focus move (a `TextEdit` whose Escape blurs it must not close the `Popup` around it with the same press) and a withdrawal ([`Self::close`]; an overlay closing on Escape must not hand it to scopes beneath) take effect at the next `resolve`, often in the same frame since action input records twice.
#[derive(Debug, Default)]
pub(super) struct Scopes {
    /// Scopes enclosing the focused widget within [`Self::active_layer`], **outermost first**; empty when nothing focused sits there.
    path: Vec<ScopeRow>,
    /// [`live_scopes`] folded once per resolve (the filter costs two `Vec` scans per row); every read of the live set goes through here.
    live: Vec<ScopeRow>,
    /// Topmost layer declaring any scope; an overlay raising it cuts off every layer beneath, on keyboard and pointer.
    active_layer: Option<Layer>,
    outermost: Option<WidgetId>,
    reader_memo: Option<ReaderMemo>,
    /// Scopes withdrawn by [`Self::close`] this frame, honoured from the next resolve (the cascade is a frame stale, so a closing overlay has already recorded its scope).
    closing: Vec<WidgetId>,
    /// The previous frame's withdrawals, still honoured since the cascade is the one that frame left.
    ///
    /// A close must outlive its pass by exactly one frame boundary: clearing per resolve lets pass B wipe pass A's close, holding longer suppresses a popup reopened under the same id. [`Self::end_frame`] swaps.
    closed: Vec<WidgetId>,
    /// The inputs the routing was last resolved from, or `None` once a withdrawal changed since.
    resolved_for: Option<ResolvedFor>,
    rebuilds: TestOnly<u32>,
}

#[derive(Copy, Clone, Debug)]
struct ResolvedFor {
    focused: Option<WidgetId>,
    cascade: CascadeKey,
}

/// The scope [`Scopes::reader`] last answered for. An app polls its chord table from one record position, so one entry makes the rest a `WidgetId` compare; [`Scopes::resolve`] clears it.
#[derive(Copy, Clone, Debug)]
struct ReaderMemo {
    parent: WidgetId,
    scope: Option<WidgetId>,
}

impl Scopes {
    /// Rebuilds the pass's routing from `focused` and the cascade; a steady frame keeps the last routing if the focus is the same, no withdrawal came, and the cascade keeps its structure ([`CascadeKey::keeps_structure`]).
    pub(super) fn resolve(&mut self, focused: Option<WidgetId>, cascade: &Cascade) {
        if let (Some(last), Some(key)) = (self.resolved_for, cascade.key)
            && last.focused == focused
            && last.cascade.keeps_structure(&key)
        {
            return;
        }
        self.resolved_for = cascade.key.map(|cascade| ResolvedFor { focused, cascade });
        self.rebuilds.bump();
        self.path.clear();
        self.reader_memo = None;
        self.live.clear();
        self.live
            .extend(live_scopes(cascade, &self.closing, &self.closed));
        self.active_layer = self.live.iter().map(|row| row.layer).max();
        if let Some(active) = self.active_layer {
            if let Some(anchor) = focused {
                self.path.extend(
                    self.live
                        .iter()
                        .filter(|row| row.layer == active && cascade.is_within(anchor, row.id)),
                );
            }
            // **Outermost, not last-recorded**, else an app root's accelerators would resolve as the nested focused text field. Subtrees are contiguous in pre-order, so a row inside the standing root is skipped and the first outside opens the next root; between sibling overlays the last recorded wins.
            self.outermost = self.live.iter().filter(|row| row.layer == active).fold(
                None,
                |root, row| match root {
                    Some(id) if cascade.is_within(row.id, id) => Some(id),
                    _ => Some(row.id),
                },
            );
        } else {
            self.outermost = None;
        }
        debug_assert!(
            self.path
                .windows(2)
                .all(|pair| cascade.is_within(pair[1].id, pair[0].id)),
            "scope path must be outermost-first — `grant` reads it as pre-order",
        );
    }

    /// Withdraws `owner` through the end of the next frame, see [`Self::closed`]; this pass keeps its routing.
    pub(super) fn close(&mut self, owner: WidgetId) {
        if !self.closing.contains(&owner) {
            self.closing.push(owner);
            self.resolved_for = None;
        }
    }

    pub(super) fn adopt_closing(&mut self, other: &Scopes) {
        for &owner in &other.closing {
            self.close(owner);
        }
    }

    /// Ages this frame's withdrawals into the next; once per frame after the last record pass.
    pub(super) fn end_frame(&mut self) {
        if !self.closed.is_empty() || !self.closing.is_empty() {
            self.resolved_for = None;
        }
        self.closed.clear();
        mem::swap(&mut self.closed, &mut self.closing);
    }

    /// Whether an overlay's scope cuts `reader`'s layer off both streams (strictly below, so its own body keeps reading).
    pub(super) fn silences(&self, reader: Layer) -> bool {
        self.active_layer
            .is_some_and(|active| active.idx() > reader.idx())
    }

    /// Whether a scope on the focused widget's path takes `class`: a claim, rather than the outermost fallback of [`Self::grant`].
    pub(super) fn path_takes(&self, class: KeyClass) -> bool {
        self.path.iter().any(|row| row.filter.takes(class))
    }

    /// [`Self::path_takes`] over only the scopes strictly inside `ancestor`.
    pub(super) fn path_takes_within(
        &self,
        class: KeyClass,
        ancestor: WidgetId,
        cascade: &Cascade,
    ) -> bool {
        self.path.iter().any(|row| {
            row.id != ancestor && cascade.is_within(row.id, ancestor) && row.filter.takes(class)
        })
    }

    /// The scope a press of `class` is granted to: the innermost on the path whose filter takes it, else the layer's outermost.
    pub(super) fn grant(&self, class: KeyClass) -> Option<WidgetId> {
        self.path
            .iter()
            .rfind(|row| row.filter.takes(class))
            .map(|row| row.id)
            .or(self.outermost)
    }

    /// Which scope a read at `parent` speaks for; outside every scope, the layer's outermost. `None` means no scope exists at all, not "silenced" (see [`Self::silences`]).
    pub(super) fn reader(
        &mut self,
        parent: Option<WidgetId>,
        cascade: &Cascade,
    ) -> Option<WidgetId> {
        let active = self.active_layer?;
        let Some(parent) = parent else {
            return self.outermost;
        };
        if let Some(memo) = self.reader_memo
            && memo.parent == parent
        {
            return memo.scope;
        }
        let scope = self
            .live
            .iter()
            .rfind(|row| row.layer == active && cascade.is_within(parent, row.id))
            .map(|row| row.id)
            .or(self.outermost);
        self.reader_memo = Some(ReaderMemo { parent, scope });
        scope
    }
}

/// The scope rows still owning input: `cascade`'s minus everything [`Scopes::close`] withdrew. Every scan goes through here, as the stale cascade lists closed overlays. Takes the withdrawal columns, not `&Scopes`, so [`Scopes::resolve`] can fill [`Scopes::live`].
fn live_scopes<'a>(
    cascade: &'a Cascade,
    closing: &'a [WidgetId],
    closed: &'a [WidgetId],
) -> impl DoubleEndedIterator<Item = ScopeRow> + 'a {
    cascade
        .scopes
        .iter()
        .copied()
        .filter(move |row| !closing.contains(&row.id) && !closed.contains(&row.id))
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::input::scope::Scopes;

    impl Scopes {
        pub(crate) const fn rebuilds(&self) -> u32 {
            self.rebuilds.count()
        }
    }
}
