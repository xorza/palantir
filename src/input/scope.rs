//! Input-scope resolution: who owns a key this pass.
//!
//! The sub-machine [`crate::input::input_state::InputState`] delegates every routing
//! question to, sibling of [`crate::input::watch::Watches`]. It holds one
//! pass's worth of derived state — the path of scopes enclosing the
//! focused widget, the layer that path sits on, and the layer's outermost
//! scope — all rebuilt by [`Scopes::resolve`] at record-pass start.
//!
//! Resolving once per pass rather than per read is what keeps grants
//! independent of where in the pass anything recorded, and it is also
//! what makes the reads cheap: everything that depends only on the
//! cascade is computed here, so [`Scopes::grant`] is a handful of bit
//! tests and [`Scopes::reader`] one containment probe per scope — and
//! that scan only on the first read at a given record position, the rest
//! of a chord table hitting [`ReaderMemo`].

use crate::input::key_class::KeyClass;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use crate::scene::cascade::entry::ScopeRow;
use crate::scene::layer::Layer;

/// This pass's resolved scope routing.
///
/// **Order is load-bearing.** The cascade appends scopes in pre-order, so
/// an ancestor chain lands outermost-first — which is why [`Self::path`]
/// needs no sort and "innermost" is `rfind` rather than a containment
/// fold. [`Self::resolve`] `debug_assert!`s the property rather than
/// trusting the cascade walk to keep it.
///
/// # Two mid-pass changes, two timings
///
/// Routing resolves once, at [`Self::resolve`]. Two things can move
/// under it before the pass ends, and they take effect at different
/// times on purpose:
///
/// - **A withdrawal ([`Self::close`]) applies immediately.** An overlay
///   that decided it is closing must stop owning input from that point
///   on, or it swallows the click that lands where it used to be. The
///   memo is dropped so reads after the withdrawal do not see the answer
///   reads before it got.
/// - **A focus move does not re-route this pass.** [`Self::path`] stays
///   as `resolve` left it, so a widget that blurs itself on a keystroke
///   does not thereby hand that same keystroke to the scope outside it —
///   a `TextEdit` whose Escape blurs it would otherwise close the
///   `Popup` around it with the one press. Focus takes effect at the
///   next `resolve`, which is the same one-frame lag every other input
///   answer carries.
///
/// The difference is which question is being asked: a withdrawal changes
/// *who exists*, a focus move changes *who is preferred*, and this pass's
/// events were already routed by the preference it had.
#[derive(Debug, Default)]
pub(super) struct Scopes {
    /// Scopes enclosing the focused widget within [`Self::active_layer`],
    /// **outermost first**. Empty when nothing focused sits in that
    /// layer, which is what [`Self::outermost`] then answers for.
    ///
    /// Capacity is retained across passes, so a stable tree resolves
    /// allocation-free.
    path: Vec<ScopeRow>,
    /// [`live_scopes`] folded once at [`Self::resolve`] time, because
    /// that pass reads it three ways — for [`Self::active_layer`], for
    /// the [`Self::path`] fill, and for the [`Self::outermost`] fold —
    /// and the filter behind it costs two `Vec` scans *per row*. All
    /// three want the same instant's rows, so one fold is not just
    /// cheaper but the more honest shape.
    ///
    /// **A snapshot, not a live view**, which is why [`Self::reader`]
    /// keeps calling [`live_scopes`] itself: [`Self::close`] lands
    /// mid-pass and has no cascade to refold this from, so reads after a
    /// close would go on seeing the withdrawn scope here.
    ///
    /// Capacity is retained across passes, like [`Self::path`].
    live: Vec<ScopeRow>,
    /// Topmost layer declaring any scope. An overlay declaring one
    /// raises this, which cuts every layer beneath it off both streams —
    /// keyboard and pointer alike, from one fact rather than a pair of
    /// per-stream claims that would have to be kept in step.
    active_layer: Option<Layer>,
    /// [`Self::active_layer`]'s outermost scope, resolved once here
    /// because it is a pure function of the cascade and every read would
    /// otherwise recompute it — one scan of the layer's scopes per pass
    /// instead of per chord.
    outermost: Option<WidgetId>,
    /// [`Self::reader`]'s last answer — see [`ReaderMemo`].
    reader_memo: Option<ReaderMemo>,
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

/// The scope [`Scopes::reader`] last answered for, and for whom.
///
/// An app polls its whole chord table from one record position, so that
/// scan repeats verbatim once per chord: it reads `parent`, the cascade,
/// and the two withdrawal columns, none of which move between two polls
/// at the same position. Holding one entry collapses a table of `n`
/// chords to one scan — the rest are a `WidgetId` compare.
///
/// Keyed on `parent` alone, because the two things that *can* invalidate
/// it both clear the memo outright: [`Scopes::resolve`] rebuilds the pass
/// and [`Scopes::close`] withdraws a scope mid-pass. Nothing else the
/// scan reads changes within a pass.
#[derive(Copy, Clone, Debug)]
struct ReaderMemo {
    parent: WidgetId,
    scope: Option<WidgetId>,
}

impl Scopes {
    /// Rebuild the pass's routing from `focused` and the cascade.
    ///
    /// `focused` is a parameter rather than read back off the input
    /// state because it is the whole input here: a scope path is a
    /// function of where focus sits and what the previous frame
    /// recorded, nothing else.
    pub(super) fn resolve(&mut self, focused: Option<WidgetId>, cascade: &Cascade) {
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
            // **Outermost, not last-recorded.** Taking the last row would
            // make an app root's accelerators resolve as the focused text
            // field nested inside it, and every one of them would die
            // mid-edit. Scopes record in pre-order, so each one's subtree
            // is a contiguous run: a row inside the standing root is a
            // descendant and skipped, and the first row outside it opens
            // the next root. The fold therefore ends on the last root,
            // which is what breaks the tie between two *sibling* overlays
            // on one layer: the last to record wins, as topmost.
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

    /// Withdraw `owner` for the rest of this frame and all of the next —
    /// see [`Self::closed`] for why that span.
    pub(super) fn close(&mut self, owner: WidgetId) {
        if !self.closing.contains(&owner) {
            self.closing.push(owner);
            // A withdrawal lands mid-pass, so reads after it must not see
            // the answer reads before it got.
            self.reader_memo = None;
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
        let scope = live_scopes(cascade, &self.closing, &self.closed)
            .rfind(|row| row.layer == active && cascade.is_within(parent, row.id))
            .map(|row| row.id)
            .or(self.outermost);
        self.reader_memo = Some(ReaderMemo { parent, scope });
        scope
    }
}

/// The scope rows still owning input: `cascade`'s, minus everything
/// [`Scopes::close`] withdrew.
///
/// **Every scan over `cascade.scopes` goes through here.** The cascade
/// is a frame stale, so it still lists closed overlays; a scan that reads
/// it raw resolves grants onto a scope that is gone and strands the
/// surviving one — silently, since the raw stream and the layer gate both
/// keep working.
///
/// Takes the two withdrawal columns rather than `&Scopes`, because
/// [`Scopes::resolve`] scans while filling [`Scopes::live`] and only a
/// field-level borrow leaves that field free.
///
/// `copied` before `filter`, so the predicate takes `&ScopeRow` rather
/// than the `&&ScopeRow` a borrowing iterator would hand it.
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
