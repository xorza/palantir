//! One drive of the frame lifecycle over a [`Ui`]: host machinery, not callable from authoring code.
//!
//! `run` classifies the frame ([`FramePlan`]), then either paints from the retained tree (`PaintOnly`) or runs `App::update`, one to three [`FrameCycle::record_pass`]es (`pre_record`, user closure, `post_record`) and `finalize_frame`; damage runs last. Per-pass resets live in [`FrameCycle::pre_record`], the once-per-frame sweep in `finalize_frame`: the question is whether the user closure re-asserts the state.

use crate::app::App;
use crate::cascade::cascade_key::CascadeKey;
use crate::common::tracy;
use crate::damage::engine::DamageInput;
use crate::damage::frame_baseline::FrameBaseline;
use crate::diagnostics::frame_stats;
use crate::display;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::node::ident::Ident;
use crate::ui::Ui;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_report::{FramePaint, FrameProcessing, FrameReport};
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::frame_runtime::{FrameClassifyInput, FramePlan};
use crate::ui::frame_stamp::FrameInput;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::window::cursor_icon::CursorIcon;
use crate::window::window_token::WindowToken;
use std::mem;

/// The host-driven half of a frame, borrowing the [`Ui`] and the [`FrameEngines`] separately so a pass can hold `&ui.forest` while writing `&mut engines.layout`.
#[derive(Debug)]
pub(super) struct FrameCycle<'a> {
    ui: &'a mut Ui,
    engines: &'a mut FrameEngines,
}

impl<'a> FrameCycle<'a> {
    pub(super) const fn new(ui: &'a mut Ui, engines: &'a mut FrameEngines) -> Self {
        Self { ui, engines }
    }

    /// Drives one application frame for `win`: runs [`App::update`] once on a fully recorded frame, then replays [`App::record`] for cold-start warmup, action input, or `request_relayout`. Paint-only frames skip both.
    pub(super) fn run<T: App>(
        mut self,
        input: FrameInput,
        win: WindowToken,
        app: &mut T,
    ) -> FrameReport {
        tracy::zone!("Ui::frame");
        let FrameInput {
            stamp,
            damage_baseline_valid,
        } = input;
        // Screened here too, at the hosts' strictness: the `internals` harness stamps a display directly, and pointer coordinates are divided by the product passes later.
        assert!(
            display::scale_factor_is_valid(stamp.display.scale_factor()),
            "{}, got {}",
            display::SCALE_RULE,
            stamp.display.scale_factor(),
        );

        let first_frame = self.ui.frame_runtime.is_first_frame();
        self.ui.frame_runtime.advance_clock(stamp.time);
        self.ui
            .frame_runtime
            .tick_text_clock(self.ui.resources.text());
        let plan = self.ui.frame_runtime.take_frame_plan(FrameClassifyInput {
            display: stamp.display,
            damage_baseline_valid,
            input_policy: self.ui.input_policy(),
            input_signal: self.ui.input.signal_since_last_frame(),
            close_requested: self.ui.close_requested(),
        });

        self.ui.frame_runtime.relayout_requested = false;
        self.ui.display = stamp.display;

        let processing = match plan {
            FramePlan::PaintOnly => {
                tracy::zone!("Ui::frame.paint_only");
                // Record payloads are not cleared: the live `tree.shapes` still index into them. PaintOnly skips `post_record`'s input cleanup, so under `OnDelta` an unrouted event can land here with the sticky arrival flag set.
                self.ui.input.drain_per_frame_queues();
                FrameProcessing::PaintOnly
            }
            FramePlan::FullRecord { .. } => {
                {
                    tracy::zone!("Ui::update_user");
                    self.ui.input.snapshot_frame_quiescent();
                    app.update(win, self.ui);
                }
                if first_frame {
                    self.warmup(win, app);
                }
                let action_flag = {
                    tracy::zone!("Ui::record_pass.A");
                    self.record_pass(win, app)
                };
                let double_layout = action_flag || self.ui.frame_runtime.relayout_requested;
                if double_layout {
                    tracy::zone!(
                        "Ui::record_pass.B",
                        text = if self.ui.frame_runtime.relayout_requested {
                            "relayout"
                        } else {
                            "action"
                        }
                    );
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

        // `ids.removed` feeds damage; PaintOnly recorded nothing, so pass an empty set.
        let surface = self.ui.display.logical_rect();
        let prev_time = self.ui.frame_runtime.prev_stamp.map(|s| s.time);
        let baseline = FrameBaseline {
            clear: self.ui.theme.window_clear,
            font_epoch: self.ui.resources.text().font_epoch(),
        };
        let input = DamageInput {
            forest: &self.ui.forest,
            cascade: &self.ui.cascade,
            surface,
            baseline,
            prev_time,
            now: self.ui.now(),
        };
        let damage = match plan {
            FramePlan::PaintOnly => self.engines.damage.compute_paint_only(input),
            FramePlan::FullRecord { force_full } => {
                self.engines
                    .damage
                    .compute(input, &self.ui.forest.ids.removed, force_full)
            }
        };

        // Re-queue the next paint-anim boundary on every path, else PaintOnly drains the ANIM wake and the caret freezes.
        if let Some(min_wake) = self.ui.forest.min_paint_anim_wake(self.ui.now()) {
            self.ui.frame_runtime.schedule_wake(
                min_wake,
                WakeReasons::ANIM,
                self.ui.display.refresh_millihertz,
            );
        }

        self.ui.frame_runtime.prev_stamp = Some(stamp);

        // The input frame boundary: held events apply against this frame's cascade; anything input owes the frame asks for it here, since no host event will.
        if self.ui.input.next_frame(&self.ui.cascade) {
            self.ui.frame_runtime.repaint_requested = true;
        }

        let report = FrameReport {
            repaint_requested: self.ui.frame_runtime.repaint_requested,
            repaint_after: self
                .ui
                .frame_runtime
                .repaint_wakes
                .first()
                .map(|w| w.deadline),
            plan: RenderPlan::from_damage(damage, baseline.clear),
            processing,
            ime_area: self.ui.window_requests.levels.ime,
        };
        // First-frame contract: with no prev snapshot every painting widget is new, so the walk can only be `Full`; and a retained-tree paint cannot be the first frame.
        debug_assert!(
            !first_frame || report.paint() == FramePaint::Full,
            "first frame must repaint in full; got {:?}",
            report.paint(),
        );
        debug_assert!(
            !first_frame || report.processing != FrameProcessing::PaintOnly,
            "first frame has no retained tree to paint from; got {:?}",
            report.processing,
        );
        report
    }

    /// Cold-start record pass, run once before frame 1 and discarded: the cascade is empty until something records, so early events hit-test against nothing.
    ///
    /// It runs against a **scratch [`InputState`]** so that:
    ///
    /// - nothing consumes the real queues (pass B's `drain_per_frame_queues` would discard a pre-frame click),
    /// - no watcher fires (`watch_*` bypasses hit-testing and would see pre-frame events twice),
    /// - no press resolves focus against an empty hit index ([`FocusPolicy`](crate::input::policy::FocusPolicy) would read it as a press on nothing).
    ///
    /// The scratch starts with the real focus, and the real input keeps what the pass asked of it (`set_focus`, `clear_focus`, `release_input_scope`). Afterwards the real input is restored and `pointer_pos` re-routed against the new cascade. Relayout/repaint requests are withdrawn, else frame 1 runs three record passes instead of two.
    ///
    /// [`InputState`]: crate::input::input_state::InputState
    fn warmup<T: App>(&mut self, win: WindowToken, app: &mut T) {
        tracy::zone!("Ui::record_pass.warmup");
        let scratch = self.ui.input.warmup_scratch();
        let saved_input = mem::replace(&mut self.ui.input, scratch);
        let _ = self.record_pass(win, app);
        let warmup_input = mem::replace(&mut self.ui.input, saved_input);
        self.ui.input.adopt_warmup(&warmup_input);
        self.ui.input.refresh_pointer_targets(&self.ui.cascade);
        self.ui.frame_runtime.relayout_requested = false;
        self.ui.frame_runtime.repaint_requested = false;
    }

    /// One `pre_record`, user record, drain action flag, `post_record` cycle; returns whether it saw action input (which triggers a second pass).
    fn record_pass<T: App>(&mut self, win: WindowToken, app: &mut T) -> bool {
        self.pre_record();
        // Synthetic viewport root for Layer::Main; else the first user node becomes the root and layout forces its rect to the surface, overriding its `Sizing` / `Sense`.
        let viewport = Widget::zstack().size(Sizing::FILL);
        let viewport_id = self.ui.resolve_ident(Ident::Verbatim(WidgetId::VIEWPORT));
        self.ui.open_node(viewport_id, &viewport.node, None);
        {
            tracy::zone!("Ui::record_user");
            app.record(win, self.ui);
        }
        let action_flag = self.ui.input.take_action_flag();
        if self.ui.debug_overlay().frame_stats {
            frame_stats::record(self.ui);
        }
        self.ui.close_node();
        self.post_record();
        action_flag
    }

    /// Opens the pass: clears everything the user closure is about to refill.
    ///
    /// All three resets are per *pass* because the closure re-asserts each; a `PaintOnly` frame runs no closure and must keep the previous values. State the closure does not re-assert belongs in `finalize_frame`'s sweep. A fourth per-pass reset goes here.
    fn pre_record(&mut self) {
        tracy::zone!("Ui::pre_record");
        self.ui.forest.pre_record();
        self.ui.input.pre_record(&self.ui.cascade);
        self.ui.window_requests.levels.cursor = CursorIcon::default();
        self.ui.window_requests.levels.ime = None;
    }

    /// Record half of a pass: finalize hashes, measure, arrange, cascade. Cascade runs here so pass B of a `request_relayout` frame reads pass A's rects via [`Ui::response_for`]. Stale cache entries can't match live keys and are reaped in `finalize_frame`.
    fn post_record(&mut self) {
        tracy::zone!("Ui::post_record");
        self.ui.forest.post_record();
        // Through `forest`, not `Ui::record_store`, which borrows all of `self.ui` while `layout.run` writes `&mut self.ui.layout`.
        let store = &self.ui.forest.record_store;
        let interned_text = store.interned_text();
        self.engines.layout.run(
            &self.ui.forest,
            &interned_text,
            self.ui.display.logical_rect(),
            &mut self.ui.layout,
        );
        let key = CascadeKey::new(
            &self.ui.forest,
            &self.ui.layout,
            self.ui.display,
            self.ui.resources.text().font_epoch(),
        );
        self.engines.cascade.run(
            &self.ui.forest,
            &self.ui.layout,
            self.ui.display,
            &key,
            &mut self.ui.cascade,
        );
        self.ui.cascade_is_last_frame = false;
    }

    /// Paint half of the frame: diffs seen ids against the last painted frame, fans `removed` out to per-widget caches, runs input and damage. Once per [`Self::run`], so a widget vanishing in pass A and returning in pass B keeps its state.
    fn finalize_frame(&mut self) {
        tracy::zone!("Ui::finalize_frame");
        let removed = self.ui.forest.ids.rollover();
        self.ui.cascade_is_last_frame = true;
        self.engines.layout.text.end_frame(removed);
        self.ui.anim.sweep_removed(removed);
        self.ui.state.sweep_removed(removed);
        self.ui.gpu_views.sweep_removed(removed);

        self.ui.input.end_frame(&self.ui.cascade);
    }
}
