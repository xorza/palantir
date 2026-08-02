//! One drive of the frame lifecycle over a [`Ui`].
//!
//! Everything here is machinery the host runs; none of it is callable
//! from authoring code. [`Ui`] keeps the widget-facing API and the
//! retained state, and hands both to a [`FrameCycle`] for the duration
//! of one [`Ui::frame`].
//!
//! ## Pass order
//!
//! `run` classifies the frame ([`FramePlan`]) and then either paints
//! from the retained tree (`PaintOnly`) or runs `App::update`, one to
//! three [`FrameCycle::record_pass`]es, and `finalize_frame`. Each
//! record pass is `pre_record` → user closure → `post_record`, where
//! `post_record` finalizes hashes, measures, arranges, and cascades.
//! Damage compute runs last, on whichever cascade the final pass left.
//!
//! ## Resets and who re-asserts them
//!
//! Three per-pass resets happen in `record_pass`, deliberately there
//! and not per frame: the input watch set, the cursor request, and the
//! forest's trees + record payloads. All three are re-asserted by the
//! closure that runs after them, so a `PaintOnly` frame — which runs no
//! closure — must keep the previous frame's values rather than reset to
//! defaults it has nobody to refill from. The retained-across-frames
//! state (clock, wake queue, cross-frame widget state, animation rows)
//! is reset nowhere and swept once per frame in `finalize_frame`.

use crate::app::App;
use crate::display;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::plan::RenderPlan;
use crate::scene::cascade::engine::cascade_fingerprint;
use crate::scene::damage::{Damage, DamageInput};
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::ui::frame::{FrameClassifyInput, FrameInput, FramePlan, WakeReasons};
use crate::ui::frame_report::{FrameProcessing, FrameReport};
use crate::ui::frame_stats;
use crate::window::{CursorIcon, WindowToken};

/// The host-driven half of a frame, borrowing the [`Ui`] it drives.
///
/// A separate type rather than more `impl Ui` because the passes reach
/// across nearly every field on `Ui` — grouping those fields would only
/// obscure the one consumer that legitimately wants all of them. This
/// keeps `Ui`'s own file to the authoring surface.
#[derive(Debug)]
pub(super) struct FrameCycle<'a> {
    ui: &'a mut Ui,
}

impl<'a> FrameCycle<'a> {
    pub(super) fn new(ui: &'a mut Ui) -> Self {
        Self { ui }
    }

    /// Drive one application frame for `win`. Runs [`App::update`] once on a
    /// fully recorded frame, then replays [`App::record`] for cold-start
    /// warmup, action input, or `request_relayout`. Paint-only frames skip
    /// both hooks. `stamp.time` is monotonic host time.
    pub(super) fn run<T: App>(
        mut self,
        input: FrameInput,
        win: WindowToken,
        app: &mut T,
    ) -> FrameReport {
        profiling::scope!("Ui::frame");
        let FrameInput {
            stamp,
            damage_baseline_valid,
        } = input;
        // Record payloads are cleared inside `record_pass` (the only path
        // that repopulates it). PaintOnly frames must NOT clear: the
        // live `tree.shapes` from last frame still references record payloads
        // contents by index (gradients, polyline points/colors, mesh
        // verts/indices, interned text spans). Clearing here would
        // leave dangling indices the encoder then dereferences.
        debug_assert!(
            display::scale_factor_is_valid(stamp.display.scale_factor),
            "Display::scale_factor must be finite and ≥ EPSILON; got {}",
            stamp.display.scale_factor,
        );

        let first_frame = self.ui.frame_runtime.prev_stamp.is_none();
        self.ui.frame_runtime.advance_clock(stamp.time);
        // Refresh the input clock so input handlers running before the
        // next frame timestamp double-clicks on this deterministic time.
        self.ui.input.frame_time = self.ui.frame_runtime.time;
        let plan = self.ui.frame_runtime.take_frame_plan(FrameClassifyInput {
            display: stamp.display,
            damage_baseline_valid,
            input_policy: self.ui.input_policy,
            input_signal: self.ui.input.signal_since_last_frame,
            close_requested: self.ui.window_frame.close_requested,
        });

        self.ui.frame_runtime.repaint_requested = false;
        self.ui.frame_runtime.relayout_requested = false;
        self.ui.display = stamp.display;

        let processing = match plan {
            FramePlan::PaintOnly => {
                profiling::scope!("Ui::frame.paint_only");
                // PaintOnly skips `record_pass` → skips `post_record`
                // → skips the input cleanup. Under `OnDelta`, an
                // unrouted event can still land here with the sticky
                // arrival flag set even though no queue accepted it.
                self.ui.input.drain_per_frame_queues();
                FrameProcessing::PaintOnly
            }
            FramePlan::FullRecord { .. } => {
                app.update(win, self.ui);
                if first_frame {
                    self.warmup(win, app);
                }
                let action_flag = {
                    profiling::scope!("Ui::record_pass.A");
                    self.record_pass(win, app)
                };
                let double_layout = action_flag || self.ui.frame_runtime.relayout_requested;
                if double_layout {
                    profiling::scope!(
                        "Ui::record_pass.B",
                        if self.ui.frame_runtime.relayout_requested {
                            "relayout"
                        } else {
                            "action"
                        }
                    );
                    // Pass B paints, regardless of any further re-record
                    // request — caps relayout at one retry per frame.
                    self.ui.input.drain_per_frame_queues();
                    let _ = self.record_pass(win, app);
                }
                self.finalize_frame();

                if double_layout {
                    FrameProcessing::DoubleLayout
                } else {
                    FrameProcessing::SingleLayout
                }
            }
        };

        self.ui.frame_runtime.note_processing(processing);

        // Damage compute reads `ids.removed` to know which widgets
        // dropped between frames. On `PaintOnly` no widgets were
        // recorded so nothing was removed — pass an empty set
        // instead of stale state from the previous frame.
        let surface = self.ui.display.logical_rect();
        let prev_time = self.ui.frame_runtime.prev_stamp.map(|s| s.time);
        let input = DamageInput {
            forest: &self.ui.forest,
            cascade: &self.ui.cascade,
            surface,
            prev_time,
            now: self.ui.frame_runtime.time,
        };
        let damage = match plan {
            FramePlan::PaintOnly => self.ui.damage_engine.compute_paint_only(input),
            FramePlan::FullRecord { force_full } => {
                self.ui
                    .damage_engine
                    .compute(input, &self.ui.forest.ids.removed, force_full)
            }
        };

        // First-frame contract: no prev snapshot to diff against, so
        // every painting widget is "new" — `damage_engine.compute`
        // must return `Damage::Full`. The walk itself is still
        // load-bearing (seeds `prev` for frame 2's incremental diff)
        // so we keep the call; the assert just pins the invariant.
        debug_assert!(
            !first_frame || matches!(damage, Damage::Full),
            "first frame must produce Damage::Full; got {damage:?}",
        );

        // Re-queue the next paint-anim boundary regardless of path.
        // FullRecord rebuilt `paint_anims.entries` during record;
        // PaintOnly retained last frame's. Either way the fold below
        // gives the next quantum boundary — without this, PaintOnly
        // drains the queued ANIM wake without replacing it and the
        // caret freezes until input forces a FullRecord.
        if let Some(min_wake) = self
            .ui
            .forest
            .min_paint_anim_wake(self.ui.frame_runtime.time)
        {
            self.ui.frame_runtime.schedule_wake(
                min_wake,
                WakeReasons::ANIM,
                self.ui.display.refresh_millihertz,
            );
        }

        self.ui.frame_runtime.prev_stamp = Some(stamp);

        FrameReport {
            repaint_requested: self.ui.frame_runtime.repaint_requested,
            repaint_after: self
                .ui
                .frame_runtime
                .repaint_wakes
                .first()
                .map(|w| w.deadline),
            plan: RenderPlan::from_damage(damage, self.ui.theme.window_clear),
            processing,
        }
    }

    /// Cold-start record pass, run once before the first frame's real
    /// one and discarded. Exists because the cascade is empty until
    /// something records into it: `on_input` events delivered between
    /// window-open and frame 1 hit-tested against nothing, and every
    /// widget would read a `None` response rect (one-frame-stale
    /// `text_edit`, `scroll`, `radio`, …) with a pointer resting on a
    /// button not hovering it until frame 2.
    ///
    /// # Contract
    ///
    /// The pass runs against a **scratch [`InputState`]**, not the real
    /// one, and this is the load-bearing part rather than a nicety:
    ///
    /// - Nothing consumes the real queues. Pass B's
    ///   `drain_per_frame_queues` would otherwise discard a click that
    ///   arrived before frame 1 — offered to a pass that cannot
    ///   hit-test it, then drained before the pass that could.
    /// - No watcher fires. `watch_*` readers bypass hit-testing, so
    ///   against real input they would observe every pre-frame event a
    ///   second time.
    /// - No press resolves focus against an empty hit index, which
    ///   [`FocusPolicy`](crate::input::policy::FocusPolicy) would read
    ///   as a press on nothing.
    ///
    /// Afterwards the real input is restored and its held `pointer_pos`
    /// re-routed against the freshly built cascade, so the visible pass
    /// records against correct hover targets. The pass's
    /// relayout/repaint requests are withdrawn: it did not happen as far
    /// as the frame gate is concerned, and leaving them set would stack
    /// the `double_layout` arm on top of the warmup for three record
    /// passes on frame 1 instead of two.
    ///
    /// [`InputState`]: crate::input::InputState
    fn warmup<T: App>(&mut self, win: WindowToken, app: &mut T) {
        profiling::scope!("Ui::record_pass.warmup");
        let saved_input = std::mem::take(&mut self.ui.input);
        let _ = self.record_pass(win, app);
        self.ui.input = saved_input;
        self.ui.input.refresh_pointer_targets(&self.ui.cascade);
        self.ui.frame_runtime.relayout_requested = false;
        self.ui.frame_runtime.repaint_requested = false;
    }

    /// One `pre_record` → user record → drain action flag → `post_record`
    /// cycle. Returns whether the cycle saw action input (which triggers
    /// a second pass in [`Self::run`]).
    fn record_pass<T: App>(&mut self, win: WindowToken, app: &mut T) -> bool {
        {
            profiling::scope!("Ui::pre_record");
            // `forest.pre_record` clears both its trees and the retained
            // payloads their shape records index into.
            self.ui.forest.pre_record();
            // Rebuild record-scoped input ownership and wake watches;
            // PaintOnly frames skip this so their last wake set persists.
            self.ui.input.begin_record(&self.ui.cascade);
            // Like the watch set, the cursor request is
            // re-asserted by whoever still wants it this pass; reset
            // here (not per frame) so PaintOnly frames keep the last
            // recorded cursor instead of flickering back to the arrow.
            self.ui.window_requests.cursor = CursorIcon::default();
        }
        // Synthetic viewport root for Layer::Main. Without this, the
        // first user-recorded node becomes the root and the layout
        // engine forces its rect to the surface — silently overriding
        // declared `Sizing` / `Sense` on the top-level widget. ZStack +
        // Fill matches the historical "root paints full surface"
        // behavior while letting user roots respect their own sizing.
        let mut viewport = Node::zstack();
        viewport.size = Some(Sizing::FILL.into());
        // Hard-coded `WidgetId::VIEWPORT` — a frame-stable parent id,
        // so top-level salts/auto ids resolve to `VIEWPORT.with(salt)`
        // like any other parent-scoped id (see `Ui::widget`).
        self.ui.forest.open_node(WidgetId::VIEWPORT, viewport, None);
        {
            profiling::scope!("Ui::record_user");
            app.record(win, self.ui);
        }
        let action_flag = self.ui.input.finish_record();
        if self.ui.resources.diagnostics.overlay.borrow().frame_stats {
            frame_stats::record(self.ui);
        }
        self.ui.forest.close_node();
        self.post_record();
        action_flag
    }

    /// Record-half of a pass: finalize hashes, run measure / arrange,
    /// then cascade. Cascade runs here (not in [`Self::finalize_frame`])
    /// so pass B of a `request_relayout` frame reads pass A's arranged
    /// rects via [`Ui::response_for`] like steady-state frames do.
    /// Stale cache entries (for widgets recorded last frame but
    /// absent this pass) are tolerated through `layout.run` — they
    /// can't match live keys — and reaped once in `finalize_frame`
    /// against the final pass's id set.
    fn post_record(&mut self) {
        profiling::scope!("Ui::post_record");
        self.ui.forest.post_record();
        let payloads = self.ui.forest.record_store.payloads.borrow();
        let interned_text = payloads.interned_text();
        self.ui.layout_engine.run(
            &self.ui.forest,
            &interned_text,
            self.ui.display.logical_rect(),
            &mut self.ui.layout,
        );
        drop(interned_text);
        drop(payloads);
        // O5 stage 0: skip the cascade when nothing feeding it changed.
        // The cascade is a pure function of subtree authoring + arranged
        // rects, and the arranged rects are determined by (subtree_hash,
        // exact surface, scroll offset/zoom) — so a matching fingerprint
        // means identical cascade output, and last frame's
        // `Ui::cascade` can be reused verbatim (the tree is rebuilt
        // with identical structure when `subtree_hash` matches, so its
        // NodeId-indexed rows still line up).
        let fp = cascade_fingerprint(&self.ui.forest, self.ui.display);
        let skip = self.ui.frame_runtime.prev_cascade_fp == Some(fp);
        self.ui.frame_runtime.note_cascade_ran(!skip);
        if skip {
            return;
        }
        self.ui.frame_runtime.prev_cascade_fp = Some(fp);
        self.ui.cascade_engine.run(
            &self.ui.forest,
            &self.ui.layout,
            self.ui.display,
            &mut self.ui.cascade,
        );
    }

    /// Paint-half of the frame: diff seen ids against the last painted
    /// frame, fan the `removed` set out to per-widget caches, run
    /// input/damage against the final pass's cascade. Sweep runs
    /// here (once per [`Self::run`]) rather than per [`Self::post_record`]
    /// so a widget that vanishes in pass A but returns in pass B keeps
    /// its state across the discard.
    fn finalize_frame(&mut self) {
        profiling::scope!("Ui::finalize_frame");
        let removed = self.ui.forest.ids.rollover();
        self.ui.layout_engine.text.end_frame(removed);
        self.ui.state.sweep_removed(removed);
        self.ui.anim.sweep_removed(removed);
        // Evict views whose widget vanished this frame; the backend frees the
        // orphaned texture the next frame it's no longer in `frame_targets`.
        // Guarded like the sweep_removed family — `retain` walks the whole
        // map even when nothing was removed.
        if !removed.is_empty() {
            self.ui.gpu_views.retain(|wid, _| !removed.contains(wid));
        }

        self.ui.input.end_frame(&self.ui.cascade);
    }
}
