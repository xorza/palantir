//! [`FrameLoop`]: what owns one window's frames, the [`Ui`] an app records through and the engines
//! only a frame runs. A host holds it, and a widget, which holds a `&mut Ui`, cannot reach it.
//!
//! [`FrameLoop::frame`] classifies the frame ([`FramePlan`]), then either paints from the retained tree (`PaintOnly`) or runs `App::update`, one to three [`FrameLoop::record_pass`]es (`pre_record`, user closure, `post_record`) and `finalize_frame`; damage runs last. Per-pass resets live in [`FrameLoop::pre_record`], the once-per-frame sweep in `finalize_frame`: the question is whether the user closure re-asserts the state.

use crate::cascade::cascade_source::CascadeSource;
use crate::cascade::engine::CascadeEngine;
use crate::common::tracy;
use crate::damage::DamageBase;
use crate::damage::engine::{DamageEngine, DamageInput};
use crate::damage::frame_baseline::FrameBaseline;
use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::display;
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::gpu::gpu_paint::gpu_views::GpuViews;
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::layout::engine::LayoutEngine;
use crate::primitives::identity::widget_id::WidgetId;
use crate::renderer::frontend::FrameScene;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::node::Node;
use crate::scene::node::id_recipe::IdRecipe;
use crate::shape::record_store::RecordStore;
use crate::ui::app::App;
use crate::ui::frame_report::{FramePaint, FrameProcessing, FrameReport};
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::frame_runtime::{FrameClassifyInput, FramePlan, RecordBaseline};
use crate::ui::frame_stats_readout;
use crate::ui::resources::UiResources;
use crate::ui::{Ui, input_ctx};
use crate::window::screenshot::{Screenshot, ScreenshotRequest};
#[cfg(feature = "winit")]
use crate::window::vsync::Vsync;
use crate::window::waker::Waker;
use crate::window::window_commands::WindowCommands;
use crate::window::window_frame_state::WindowFacts;
use crate::window::window_output::WindowOutput;
use crate::window::window_token::WindowToken;
use std::mem;
use std::time::Duration;

/// The scene `ui` holds, apart from the engines, so a caller can write those as it reads it.
const fn scene_of(ui: &Ui) -> FrameScene<'_> {
    FrameScene {
        forest: &ui.forest,
        layout: &ui.layout,
        cascade: &ui.cascade,
        stamp: ui.frame_runtime.stamp(),
    }
}

/// The parts of a [`FrameLoop`] a submit borrows at once ([`FrameLoop::submit_parts`]).
#[derive(Debug)]
pub(crate) struct SubmitParts<'a> {
    pub(crate) stats: &'a mut GpuPassStats,
    pub(crate) store: &'a RecordStore,
    pub(crate) gpu_views: &'a GpuViews,
}

/// One window's frames: its [`Ui`], and the engines that run over it with the retained caches each
/// needs to run incrementally (measure cache, previous cascade, last paint snapshot). The engines
/// sit beside the `Ui`, so a pass can hold `&ui.forest` while it writes `&mut layout`, and so a
/// widget cannot reach them. The host's side of the window's seam is here for the same reason. This
/// is the shape of Masonry's `RenderRoot`, the composition root a host gives events and asks to
/// redraw.
///
/// Registers its token in the window directory for as long as it lives, so no close path can leave
/// `Ui::is_window_open` true.
#[derive(Debug)]
pub(crate) struct FrameLoop {
    pub(crate) ui: Ui,
    /// Measure/arrange, plus the measure cache and the `TextSystem` whose clock ages the glyph atlas.
    pub(crate) layout: LayoutEngine,
    pub(crate) cascade: CascadeEngine,
    /// Retains the previous frame's paint snapshot, making the damage diff incremental.
    pub(crate) damage: DamageEngine,
    /// The window the app records for.
    token: WindowToken,
}

impl Drop for FrameLoop {
    fn drop(&mut self) {
        self.ui.resources.windows().remove(self.token);
    }
}

impl FrameLoop {
    /// The frames of the window `token`, over the shared `resources`. Pass the same loop every
    /// frame: a new one discards the measure cache, cascade and damage baseline, forcing a full
    /// repaint.
    pub(crate) fn new(token: WindowToken, resources: &UiResources) -> Self {
        resources.windows().add(token);
        Self {
            ui: Ui::new(resources.clone()),
            layout: LayoutEngine::new(resources.text().clone()),
            cascade: CascadeEngine::default(),
            damage: DamageEngine::default(),
            token,
        }
    }

    /// The window the app records for.
    #[cfg(any(test, feature = "winit"))]
    pub(crate) const fn token(&self) -> WindowToken {
        self.token
    }

    /// Feed an event that arrived at `now`. The winit host stamps it on an input clock
    /// of its own, which runs on while a hidden window's frame clock skips the span,
    /// so two presses either side never read as a double click; the offscreen host
    /// stamps it from the source its frames read. The [`InputDelta`] says whether
    /// to redraw.
    #[inline]
    pub(crate) fn on_input(&mut self, event: InputEvent<'_>, now: Duration) -> InputDelta {
        self.ui
            .input
            .on_input(event, input_ctx(&self.ui.cascade, &self.ui.resources), now)
    }

    /// The window's host sets the waker [`Ui::waker`] hands out.
    pub(crate) fn set_waker(&mut self, waker: Waker) {
        self.ui.host.waker = waker;
    }

    /// Publish this frame's window-manager facts. Asserts the previous close veto
    /// did not survive; [`Self::drain_window_output`] clears it.
    #[cfg(feature = "winit")]
    pub(crate) fn set_window_facts(&mut self, facts: WindowFacts) {
        debug_assert!(
            !self.ui.host.window_requests.close_vetoed,
            "a veto outlived the frame that raised it",
        );
        self.ui.host.window_frame.facts = facts;
    }

    /// Seed the pacing level from the swapchain the host opened, so a control
    /// writing its value back does not override an explicit present mode.
    #[cfg(feature = "winit")]
    #[inline]
    pub(crate) const fn seed_vsync(&mut self, vsync: Vsync) {
        self.ui.host.window_requests.levels.vsync = vsync;
    }

    /// Drain this frame's window scratch into `commands` and return the levels the host applies
    /// afterwards; see
    /// [`WindowRequests::drain`](crate::window::window_requests::WindowRequests::drain).
    pub(crate) fn drain_window_output(&mut self, commands: &mut WindowCommands) -> WindowOutput {
        let close_requested = self.ui.host.window_frame.facts.close_requested;
        let levels = self
            .ui
            .host
            .window_requests
            .drain(self.token, close_requested, commands);
        self.ui.host.window_frame.facts = WindowFacts::default();
        levels
    }

    /// Move the images this frame asked for onto `into`, as the driver takes them once the frame
    /// is recorded.
    pub(crate) fn take_screenshot_requests(&mut self, into: &mut Vec<ScreenshotRequest>) {
        into.append(&mut self.ui.host.window_requests.screenshots);
    }

    /// Hand the next frame the image that landed for it, or none.
    pub(crate) fn set_screenshot_taken(&mut self, screenshot: Option<Screenshot>) {
        if screenshot.is_some() {
            self.ui.frame_runtime.request_repaint();
        }
        self.ui.host.window_frame.screenshot = screenshot.map(Box::new);
    }

    /// What the frontend builds the last frame's draw list from.
    pub(crate) const fn frame_scene(&self) -> FrameScene<'_> {
        scene_of(&self.ui)
    }

    /// Every `GpuView` this window records, which its frames paint through.
    pub(crate) const fn gpu_views(&self) -> &GpuViews {
        &self.ui.memory.gpu_views
    }

    /// What a submit of the last frame reads, and where it publishes the window's GPU samples,
    /// borrowed at once.
    pub(crate) fn submit_parts(&mut self) -> SubmitParts<'_> {
        SubmitParts {
            stats: &mut self.ui.host.gpu_pass_stats,
            store: &self.ui.forest.record_store,
            gpu_views: &self.ui.memory.gpu_views,
        }
    }

    /// Whether the host snaps axis-aligned paint edges to physical pixels, in every window.
    pub(crate) fn pixel_snap(&self) -> bool {
        self.ui.resources.settings().pixel_snap()
    }

    /// The scale the current cascade was laid out at; a host divides event
    /// positions by it. `None` before the first frame.
    #[cfg(any(test, feature = "winit"))]
    pub(crate) fn laid_out_scale(&self) -> Option<f32> {
        self.ui
            .frame_runtime
            .prev_stamp
            .map(|stamp| stamp.display.scale_factor())
    }

    /// Set the last frame's stamp. `Some` makes the next frame warm (skips warmup); `None` is a
    /// cold start.
    pub(crate) const fn set_prev_stamp(&mut self, stamp: Option<FrameStamp>) {
        self.ui.frame_runtime.set_prev_stamp(stamp);
    }

    /// Replace the display the next frame lays out at, as the window driver does.
    pub(crate) const fn set_display(&mut self, display: Display) {
        self.ui.frame_runtime.set_display(display);
    }

    /// Drives one application frame: runs [`App::update`] once on a fully recorded frame, then replays [`App::record`] for cold-start warmup, action input, or `request_relayout`. Paint-only frames skip both. `damage_base` says whether last frame's damage snapshot still describes the surface.
    pub(crate) fn frame<T: App>(
        &mut self,
        stamp: FrameStamp,
        damage_base: DamageBase,
        app: &mut T,
    ) -> FrameReport {
        tracy::zone!("Ui::frame");
        debug_assert!(
            display::scale_factor_is_valid(stamp.display.scale_factor()),
            "SystemScale's range keeps the product with any UserScale valid, got {}",
            stamp.display.scale_factor(),
        );

        let first_frame = self.ui.frame_runtime.is_first_frame();
        let device_epoch = self.ui.resources.device().epoch();
        self.ui.frame_runtime.begin_frame(stamp);
        self.ui.advance_theme();
        self.ui.frame_runtime.transparency = self.ui.resources.settings().transparency();
        self.ui
            .frame_runtime
            .tick_text_clock(self.ui.resources.text());
        let record_baseline = RecordBaseline {
            theme_epoch: self.ui.window_theme.epoch(),
            font_epoch: self.ui.resources.text().font_epoch(),
            device_epoch,
            transparency: self.ui.frame_runtime.transparency,
        };
        let plan = self.ui.frame_runtime.take_frame_plan(FrameClassifyInput {
            display: stamp.display,
            record_baseline,
            damage_base,
            input_policy: self.ui.input_policy(),
            input_signal: self.ui.input.signal_since_last_frame(),
            close_requested: self.ui.close_requested(),
        });

        // After the plan, which spends the requests made before it on this frame.
        if self.ui.window_theme.is_moving() {
            self.ui.frame_runtime.request_repaint();
        }
        self.ui.frame_runtime.begin_pass();

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
                    app.update(self.token, &self.ui);
                }
                if first_frame {
                    self.warmup(app);
                }
                let action_flag = {
                    tracy::zone!("Ui::record_pass.A");
                    self.record_pass(app)
                };
                // A modal in pass A cancelled an overlay already recorded there, which no record
                // can take back, as an app decides what to show only as it records. Pass B paints
                // it closed; left to the next frame, it would stay over the modal until some input.
                // Rare: only an app that records the modal after the overlay takes this.
                let cancel = self.ui.overlays.needs_pass();
                let double_layout =
                    action_flag || self.ui.frame_runtime.relayout_requested() || cancel;
                if double_layout {
                    tracy::zone!(
                        "Ui::record_pass.B",
                        text = if self.ui.frame_runtime.relayout_requested() {
                            "relayout"
                        } else if cancel {
                            "overlay cancel"
                        } else {
                            "action"
                        }
                    );
                    self.ui.input.drain_per_frame_queues();
                    self.ui.frame_runtime.begin_pass();
                    let _ = self.record_pass(app);
                    // A modal that first recorded in pass B cancelled an overlay past the frame's
                    // last pass: the next frame applies it.
                    if self.ui.overlays.needs_pass() {
                        self.ui.frame_runtime.request_repaint();
                    }
                }
                // Pass B is the frame's last, so a relayout it asks for takes the next frame.
                // Refused: dropping the ask, which leaves the frame stale until input.
                if self.ui.frame_runtime.carry_relayout() {
                    self.ui.frame_runtime.request_repaint();
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
        let surface = self.ui.frame_runtime.stamp().display.logical_rect();
        let prev_time = self.ui.frame_runtime.prev_stamp.map(|s| s.time);
        let baseline = FrameBaseline {
            clear: self.ui.theme().window_bg,
            font_epoch: self.ui.resources.text().font_epoch(),
            collision_overlay: !self.ui.forest.collisions.is_empty(),
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
            FramePlan::PaintOnly => self.damage.compute_paint_only(input),
            FramePlan::FullRecord { base } => {
                self.damage
                    .compute(input, &self.ui.forest.ids.removed, base)
            }
        };

        // Re-queue the next paint-anim boundary on every path, else PaintOnly drains the ANIM wake and the caret freezes.
        if let Some(min_wake) =
            self.ui
                .cascade
                .min_paint_animation_wake(&self.ui.forest, surface, self.ui.now())
        {
            self.ui.frame_runtime.schedule_wake(
                min_wake,
                WakeReasons::ANIMATION,
                self.ui.frame_runtime.stamp().display.refresh_millihertz,
            );
        }

        self.ui.frame_runtime.end_frame();

        // The input frame boundary: held events apply against this frame's cascade; anything input owes the frame asks for it here, since no host event will.
        let ctx = input_ctx(&self.ui.cascade, &self.ui.resources);
        if self.ui.input.next_frame(ctx) {
            self.ui.frame_runtime.request_repaint();
        }

        let report = FrameReport {
            repaint_requested: self.ui.frame_runtime.repaint_requested(),
            repaint_at: self.ui.frame_runtime.next_wake(),
            plan: damage.map(|damage| RenderPlan {
                clear: baseline.clear,
                damage,
            }),
            processing,
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
    /// The scratch starts with the real focus, and the real input keeps what the pass asked of it (`set_focus`, `clear_focus`, `withdraw_input_scope`). Afterwards the real input is restored and `pointer_pos` re-routed against the new cascade. Relayout/repaint requests are withdrawn, else frame 1 runs three record passes instead of two.
    ///
    /// [`InputState`]: crate::input_state::InputState
    fn warmup<T: App>(&mut self, app: &mut T) {
        tracy::zone!("Ui::record_pass.warmup");
        let scratch = self.ui.input.warmup_scratch();
        let saved_input = mem::replace(&mut self.ui.input, scratch);
        let _ = self.record_pass(app);
        let warmup_input = mem::replace(&mut self.ui.input, saved_input);
        self.ui.input.adopt_warmup(&warmup_input);
        self.ui.input.refresh_pointer_targets(&self.ui.cascade);
        self.ui.frame_runtime.withdraw_requests();
    }

    /// One `pre_record`, user record, drain action flag, `post_record` cycle; returns whether it saw action input (which triggers a second pass).
    fn record_pass<T: App>(&mut self, app: &mut T) -> bool {
        let action_flag = self.record_user(app);
        self.post_record();
        action_flag
    }

    /// The record half of a pass before `post_record`: `pre_record`, then the user closure under the viewport root; returns whether it saw action input.
    fn record_user<T: App>(&mut self, app: &mut T) -> bool {
        self.pre_record();
        // Synthetic viewport root for Layer::Main; else the first user node becomes the root and layout forces its rect to the surface, overriding its `Sizing` / `Sense`.
        let viewport_id = self
            .ui
            .resolve_ident(IdRecipe::Verbatim(WidgetId::VIEWPORT));
        self.ui.open_node(viewport_id, &Node::viewport(), None);
        {
            tracy::zone!("Ui::record_user");
            app.record(self.token, &mut self.ui);
        }
        let action_flag = self.ui.input.take_action_flag();
        if self.ui.debug_overlay().frame_stats {
            frame_stats_readout::record(&mut self.ui);
        }
        self.ui.close_node();
        action_flag
    }

    /// Opens the pass: clears everything the user closure is about to refill.
    ///
    /// The first three resets are per *pass* because the closure re-asserts each; a `PaintOnly` frame runs no closure and must keep the previous values. State the closure does not re-assert belongs in `finalize_frame`'s sweep. The overlay stack steps a modal's cancel on by one pass, so a `PaintOnly` frame leaves it for the next pass that records. A further per-pass reset goes here.
    fn pre_record(&mut self) {
        tracy::zone!("Ui::pre_record");
        self.ui
            .forest
            .pre_record(self.ui.frame_runtime.transparency);
        self.ui.input.pre_record(&self.ui.cascade);
        self.ui.host.window_requests.begin_pass();
        self.ui.overlays.begin_pass();
    }

    /// Record half of a pass: finalize hashes, measure, arrange, cascade. Cascade runs here so pass B of a `request_relayout` frame reads pass A's rects via [`Ui::response_for`]. Stale cache entries can't match live keys and are reaped in `finalize_frame`.
    fn post_record(&mut self) {
        tracy::zone!("Ui::post_record");
        self.ui.forest.post_record();
        self.layout.run(
            &self.ui.forest,
            self.ui.frame_runtime.stamp().display.logical_rect(),
            &mut self.ui.layout,
        );
        self.cascade.run(
            CascadeSource {
                forest: &self.ui.forest,
                layout: &self.ui.layout,
                display: self.ui.frame_runtime.stamp().display,
                font_epoch: self.ui.resources.text().font_epoch(),
            },
            &mut self.ui.cascade,
        );
    }

    /// Paint half of the frame: diffs seen ids against the last painted frame, fans `removed` out to per-widget caches, runs input and damage. Once per [`Self::frame`], so a widget vanishing in pass A and returning in pass B keeps its state.
    fn finalize_frame(&mut self) {
        tracy::zone!("Ui::finalize_frame");
        let removed = self.ui.forest.ids.rollover();
        self.layout.end_frame(removed);
        self.ui
            .memory
            .sweep(removed, self.ui.frame_runtime.render_frame_id);

        self.ui.input.end_frame(&self.ui.cascade);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::primitives::geometry::rect::Rect;
    use crate::ui::app::App;
    use crate::ui::frame_loop::{FrameLoop, scene_of};

    impl FrameLoop {
        /// One record pass of `app` alone, its hashes finalized: no layout, cascade, damage or
        /// frame sweep, so a bench times record apart from the stages after it.
        pub(crate) fn record_alone<T: App>(&mut self, app: &mut T) {
            let _ = self.record_user(app);
            self.ui.forest.post_record();
        }

        /// Mark as stale each pane whose footprint a rect of `changed` touches, for a bench that
        /// forces a frame's decision.
        pub(crate) fn force_stale_panes(&mut self, changed: &[Rect]) {
            for pane in scene_of(&self.ui).panes() {
                if changed.iter().any(|rect| rect.intersects(pane.footprint)) {
                    self.damage.force_stale_pane(pane.key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::host::harness::Harness;
    use crate::ui::frame_loop::FrameLoop;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;

    /// A window stays open for exactly as long as its frames live.
    #[test]
    fn the_window_directory_holds_a_token_while_its_loop_lives() {
        let h = Harness::mono(UVec2::ONE);
        let token = WindowToken(7);
        let resources = h.frames().ui.resources.clone();
        assert!(!h.ui().is_window_open(token));
        let frames = FrameLoop::new(token, &resources);
        assert!(h.ui().is_window_open(token));
        assert_eq!(frames.token(), token);
        drop(frames);
        assert!(!h.ui().is_window_open(token));
    }
}
