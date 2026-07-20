//! `WindowDriver` — the target-agnostic render core one host target owns above
//! the shared [`WgpuBackend`](crate::renderer::backend::WgpuBackend): its [`Ui`]
//! recorder, a per-target [`Frontend`] (CPU encode/compose scratch), the
//! persistent [`Backbuffer`] (the target's last-frame pixels), and the
//! per-target frame clock.
//!
//! What every window shares splits two ways: the GPU resources — render
//! pipelines, glyph + gradient atlases, the image texture cache, and renderer
//! device/queue handles — live on the **one** shared `WgpuBackend` the host
//! passes into every method; render assets, text shaping, diagnostics, and the
//! window directory derive from [`HostShared`] capability views. Each `Ui` owns
//! its per-window record store alongside its tree. So N windows render through
//! one GPU renderer without sharing frame-local geometry.
//!
//! [`WindowDriver::cpu_frame`] freezes the frame and builds the draw list;
//! [`WindowDriver::render_to_texture`] submits it to any caller-owned texture.
//! [`crate::OffscreenHost`] drives those operations directly, while the winit
//! adapter owns swapchain acquisition, presentation, occlusion, and wake
//! scheduling.

use crate::app::App;
use crate::host::clock::{Clock, RealtimeClock};
use crate::host::shared::HostShared;
use crate::renderer::backend::{Backbuffer, Stencil, Submission, SubmissionTargets, WgpuBackend};
use crate::renderer::frontend::Frontend;
use crate::ui::Ui;
use crate::ui::damage::FULL_REPAINT_THRESHOLD;
use crate::ui::frame::{FrameInput, FrameStamp};
use crate::ui::frame_report::{RenderKind, RenderPlan};
use crate::window::WindowToken;
use crate::{Display, FrameReport};

/// Per-window state driving the shared [`WgpuBackend`]. Built by
/// [`WindowDriverBuilder`] from the shared [`HostShared`]; owns no GPU
/// resources except its own [`Backbuffer`] + [`Stencil`].
#[derive(Debug)]
pub(crate) struct WindowDriver {
    /// Stable application identity for this render stream. Stored here so a
    /// retained `Ui` cannot be driven under a different token on a later frame.
    pub(crate) token: WindowToken,
    pub(crate) ui: Ui,
    /// Per-window CPU encode/compose scratch with its own retained
    /// `RenderBuffer` — this window's draw list.
    frontend: Frontend,
    /// Persistent off-screen color target holding last frame's pixels for
    /// `LoadOp::Load` partial damage. Used by `BackbufferCopy` every frame and
    /// by `DirectAdaptive` for its small-partial path (paint the damage region,
    /// then copy out). A `DirectAdaptive` window that only ever paints full
    /// frames never allocates it. Created lazily on the first frame that needs
    /// it, recreated on resize / format change.
    backbuffer: Option<Backbuffer>,
    /// `true` when [`Self::backbuffer`] mirrors what's currently on the target
    /// (the last presented frame went through it), so a `DirectAdaptive` small
    /// partial can `LoadOp::Load` it and paint just the damage region. A direct
    /// full frame bypasses the backbuffer, leaving it stale (`false`) — the next
    /// partial then resyncs it with one full repaint before cheap partials
    /// resume. Irrelevant to `BackbufferCopy` (every frame touches the
    /// backbuffer, so it always stays fresh).
    backbuffer_fresh: bool,
    /// Whether the last frame completed the presentation action selected by
    /// this driver. Invalid while a paint/copy is pending or after target
    /// invalidation, so the next UI frame discards its prior damage baseline.
    output_valid: bool,
    /// This window's rounded-clip stencil attachment — allocated lazily,
    /// resized to the target. Separate from `backbuffer` so the direct-present
    /// path can have a stencil without a backbuffer color texture.
    stencil: Option<Stencil>,
    /// How this window's frames reach the target — see [`PresentStrategy`].
    strategy: PresentStrategy,
    /// Per-frame time source — `clock.now()` feeds `Ui::frame` each call.
    /// Injected at construction ([`RealtimeClock`](crate::host::clock::RealtimeClock)
    /// for on-screen windows, [`FixedClock`](crate::host::clock::FixedClock) for a
    /// reproducible offscreen render) so the pipeline doesn't branch on it.
    pub(crate) clock: Box<dyn Clock>,
    /// Whether axis-aligned paint edges snap to physical pixels.
    pub(crate) pixel_snap: bool,
}

/// How a window's frames reach its target, chosen per host at construction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PresentStrategy {
    /// A target whose prior contents can't be relied on — a fresh texture each
    /// call (screenshots, the visual harness). Every frame renders into the
    /// persistent backbuffer and copies out, so the whole target is filled
    /// regardless of its prior contents; skip frames copy the backbuffer.
    BackbufferCopy,
    /// A direct-present swapchain target, where the host owns skip frames. Full
    /// frames repaint straight into the target (no copy); small partials paint
    /// just the damage region into the backbuffer and copy it out (cheaper than
    /// repainting the whole surface); a near-full partial is promoted to a
    /// direct full repaint. A direct frame leaves the backbuffer stale, so the
    /// next partial resyncs it with one full repaint before cheap partials
    /// resume.
    DirectAdaptive,
}

/// How a frame reaches the target, given its plan and the window's
/// [`PresentStrategy`]. Computed once in `cpu_frame` — which builds the
/// draw list for it — and threaded through to the GPU half, so the
/// submitted plan is by construction the one the draw list was built for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PresentMode {
    /// Skip frame on a backbuffer-copy window: copy the backbuffer onto the
    /// target so it's filled regardless of its prior contents.
    SkipCopy,
    /// Skip frame on a direct-present window: the host owns the skip, so there
    /// is no target to update.
    SkipNoop,
    /// Full repaint rendered directly into the target — no backbuffer copy.
    Direct(RenderPlan),
    /// Render the plan into the backbuffer, then copy it onto the target.
    ViaBackbuffer(RenderPlan),
}

/// Coverage fraction above which [`PresentStrategy::DirectAdaptive`] promotes a
/// `Partial` to a direct full repaint instead of painting just the damage region
/// into the backbuffer and copying out. Read against the region's sealed
/// `coverage` — the same axis as the damage engine's [`FULL_REPAINT_THRESHOLD`],
/// and strictly below it (a promoted partial must still reach here *as* a
/// `Partial`, not already collapsed to `Full`; enforced by the assert below).
///
/// The backbuffer path pays a *fixed* whole-surface copy every frame regardless
/// of damage size, on top of re-shading every leaf the region intersects. Once a
/// partial touches enough geometry that its paint + copy approaches a plain full
/// repaint, going direct (which drops the copy) wins. Empirically the crossover
/// sits near 0.40 on the bandwidth-bound `frame` bench (Radeon 680M): the
/// `scrolling` arm shifts a panel transform so ~half the surface damages, yet the
/// band crosses dense scrolled content — 7.8 ms via backbuffer vs 6.8 ms direct.
/// Sub-threshold partials (the `partial` arm's footer counter is ~0.04 %) stay on
/// the backbuffer path, where a tiny re-shade + one copy (3.3 ms) beats a
/// whole-surface direct repaint (6.8 ms). Area is a proxy for paint cost, not a
/// measurement of it, so the line sits a little under the known-expensive scroll
/// band rather than at a precise break-even.
const DIRECT_PROMOTE_COVERAGE: f32 = 0.4;

// A promoted partial must still reach `present_mode` *as* a `Partial`, never
// collapsed to `Full` by the damage engine first — so the promote point stays
// strictly below `FULL_REPAINT_THRESHOLD`. Compile-time guard: retuning either
// past the other fails the build instead of silently killing promotion.
const _: () = assert!(DIRECT_PROMOTE_COVERAGE < FULL_REPAINT_THRESHOLD);

fn present_mode(
    plan: Option<RenderPlan>,
    strategy: PresentStrategy,
    backbuffer_fresh: bool,
) -> PresentMode {
    match strategy {
        PresentStrategy::DirectAdaptive => match plan {
            // Swapchain skips never acquire a target because the host owns them.
            None => PresentMode::SkipNoop,
            Some(p) => match p.kind {
                // Already a whole-surface repaint — straight into the target.
                RenderKind::Full => PresentMode::Direct(p),
                RenderKind::Partial { region } => {
                    // `region.coverage` was sealed when the damage engine built
                    // this region (`collapse_from`); see `DIRECT_PROMOTE_COVERAGE`.
                    if region.coverage > DIRECT_PROMOTE_COVERAGE {
                        // Large partial: skip the copy, repaint direct.
                        PresentMode::Direct(p.to_full())
                    } else if backbuffer_fresh {
                        // Backbuffer mirrors the target: paint just the damage
                        // region into it and copy out.
                        PresentMode::ViaBackbuffer(p)
                    } else {
                        // Backbuffer went stale after a direct frame: resync it
                        // with one full repaint before cheap partials resume.
                        PresentMode::ViaBackbuffer(p.to_full())
                    }
                }
            },
        },
        // Fresh target each call: render the plan into the backbuffer and copy
        // it out so the whole target is filled regardless of its prior contents.
        PresentStrategy::BackbufferCopy => match plan {
            None => PresentMode::SkipCopy,
            Some(p) => PresentMode::ViaBackbuffer(p),
        },
    }
}

/// The CPU half's result: the host-facing report plus the [`PresentMode`]
/// sealed at draw-list-build time. Threading the mode (rather than
/// recomputing it in the GPU half) is what guarantees the submitted plan
/// is the one the draw list was built for.
#[derive(Debug)]
pub(crate) struct CpuFrame {
    pub(crate) report: FrameReport,
    pub(crate) mode: PresentMode,
}

/// Seals per-window policy before allocating the recorder and frontend.
#[derive(Debug)]
pub(crate) struct WindowDriverBuilder<'a> {
    token: WindowToken,
    shared: &'a HostShared,
    max_texture_dim: u32,
    strategy: PresentStrategy,
    clock: Box<dyn Clock>,
    pixel_snap: bool,
}

impl WindowDriverBuilder<'_> {
    pub(crate) fn strategy(mut self, strategy: PresentStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub(crate) fn clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub(crate) fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    pub(crate) fn build(self) -> WindowDriver {
        WindowDriver {
            token: self.token,
            ui: Ui::new(self.shared.ui_shared()),
            frontend: Frontend::new(self.max_texture_dim),
            backbuffer: None,
            backbuffer_fresh: false,
            output_valid: false,
            stencil: None,
            strategy: self.strategy,
            clock: self.clock,
            pixel_snap: self.pixel_snap,
        }
    }
}

impl WindowDriver {
    /// Start building a driver for `token` from the shared [`HostShared`].
    /// Its `Ui` receives the render, diagnostics, and window-directory
    /// capabilities plus a fresh per-window record store.
    /// `max_texture_dim` is the device's fixed texture limit used to cap
    /// `GpuView` targets. Defaults suit a swapchain window: direct adaptive
    /// presentation, realtime clock, and physical-pixel snapping.
    pub(crate) fn builder(
        token: WindowToken,
        shared: &HostShared,
        max_texture_dim: u32,
    ) -> WindowDriverBuilder<'_> {
        WindowDriverBuilder {
            token,
            shared,
            max_texture_dim,
            strategy: PresentStrategy::DirectAdaptive,
            clock: Box::new(RealtimeClock::new()),
            pixel_snap: true,
        }
    }

    /// Invalidate all state whose correctness depends on the current render
    /// target. Target-owning adapters call this when their size or format key
    /// changes, before running the next CPU frame.
    pub(crate) fn invalidate_target(&mut self) {
        self.output_valid = false;
        self.backbuffer_fresh = false;
    }

    /// The shared CPU half: app lifecycle → record / measure / arrange /
    /// cascade / damage followed, when the frame actually paints, by the
    /// draw-list build (encode → compose → resolve `GpuView`s into the
    /// frontend's buffer). Seals the [`PresentMode`] here — the one place it
    /// is computed — so the GPU half submits exactly the plan the draw list
    /// was built for (a promoted or resync'd Partial builds its escalated Full
    /// list). No GPU input — the `GpuView` size cap was captured on the
    /// `Frontend` at construction. Shared by the offscreen and surface
    /// adapters.
    #[profiling::function]
    pub(crate) fn cpu_frame<T: App>(&mut self, display: Display, app: &mut T) -> CpuFrame {
        let report = self.ui.frame(
            FrameInput {
                stamp: FrameStamp::new(display, self.clock.now()),
                damage_baseline_valid: self.output_valid,
            },
            self.token,
            app,
        );
        self.finish_cpu_frame(report)
    }

    fn finish_cpu_frame(&mut self, report: FrameReport) -> CpuFrame {
        let mode = present_mode(report.plan, self.strategy, self.backbuffer_fresh);
        if !matches!(mode, PresentMode::SkipNoop) {
            self.output_valid = false;
        }
        // Build the draw list now (CPU) when the frame paints — encode,
        // compose, and resolve `GpuView` targets from the frozen scene.
        // Skip frames build nothing.
        if let PresentMode::Direct(plan) | PresentMode::ViaBackbuffer(plan) = mode {
            self.frontend.build(self.ui.frame_scene(), plan);
        }
        CpuFrame { report, mode }
    }

    /// GPU submit against a caller-supplied texture, through the shared
    /// `backend`, dispatching on the [`PresentMode`] `cpu_frame` sealed. On
    /// [`PresentMode::SkipCopy`], copies the persistent backbuffer onto
    /// `target` so callers that always present still see valid pixels.
    /// Shared by the offscreen and surface adapters.
    #[profiling::function]
    pub(crate) fn render_to_texture(
        &mut self,
        backend: &mut WgpuBackend,
        target: &wgpu::Texture,
        mode: PresentMode,
    ) {
        let size = target.size();
        let display_phys = self.ui.display.physical;
        debug_assert!(
            size.width == display_phys.x && size.height == display_phys.y,
            "render_to_texture: target size {}x{} doesn't match the display physical \
             size ({}x{}) that `cpu_frame` ran against — scissor / viewport math \
             would be off. Update `Display.physical` on resize before the next \
             `cpu_frame`.",
            size.width,
            size.height,
            display_phys.x,
            display_phys.y,
        );
        // The CPU phase already composed `GpuView`s into
        // `self.frontend.buffer.frame_targets` (callback + raster target — see
        // `cpu_frame`); this is GPU submit only.
        let debug_overlay = self.ui.debug_overlay();
        // Rounded-clip stencil, shared by both paint paths and sized to the
        // target. Gated to them: on a skip frame the frontend didn't build,
        // so `buffer.rounded_clips` is stale and no pass reads the stencil.
        let stencil_view = match mode {
            PresentMode::Direct(_) | PresentMode::ViaBackbuffer(_)
                if !self.frontend.buffer.rounded_clips.is_empty() =>
            {
                backend.ensure_stencil(&mut self.stencil, size);
                Some(&self.stencil.as_ref().expect("ensure_stencil ran").view)
            }
            _ => None,
        };
        match mode {
            // Nothing changed and the target already holds the last render —
            // leave it untouched.
            PresentMode::SkipNoop => self.output_valid = true,
            PresentMode::SkipCopy => {
                // A `Skip` implies the previous frame painted at this size +
                // format, so the backbuffer must exist (and match — the
                // backend asserts that).
                let bb = self
                    .backbuffer
                    .as_ref()
                    .expect("SkipCopy implies a prior submitted paint frame");
                backend.copy_backbuffer_to_surface(bb, target);
                self.output_valid = true;
            }
            // Full repaint straight into the target — no backbuffer at all, so
            // it goes stale: the next partial must resync it first.
            PresentMode::Direct(plan) => {
                let payloads = self.ui.record_store.payloads.borrow();
                backend.submit(Submission {
                    targets: SubmissionTargets {
                        surface: target,
                        backbuffer: None,
                        stencil: stencil_view,
                    },
                    payloads: &payloads,
                    buffer: &self.frontend.buffer,
                    plan,
                    debug_overlay,
                });
                self.backbuffer_fresh = false;
                self.output_valid = true;
            }
            // Render into the backbuffer and copy it out; the backbuffer now
            // mirrors the target.
            PresentMode::ViaBackbuffer(plan) => {
                let recreated =
                    backend.ensure_backbuffer(&mut self.backbuffer, size, target.format());
                // A Partial reaches here un-escalated only when
                // `backbuffer_fresh` — last frame rendered into the backbuffer
                // at this size/format — so a recreate under Partial means the
                // freshness invariant broke. Escalating here couldn't fix it:
                // the draw list was already Partial-culled in `cpu_frame`.
                debug_assert!(
                    !recreated || matches!(plan.kind, RenderKind::Full),
                    "backbuffer (re)created under a Partial plan whose draw \
                     list was culled for Partial"
                );
                let payloads = self.ui.record_store.payloads.borrow();
                backend.submit(Submission {
                    targets: SubmissionTargets {
                        surface: target,
                        backbuffer: self.backbuffer.as_ref(),
                        stencil: stencil_view,
                    },
                    payloads: &payloads,
                    buffer: &self.frontend.buffer,
                    plan,
                    debug_overlay,
                });
                self.backbuffer_fresh = true;
                self.output_valid = true;
            }
        }
    }
}

#[cfg(test)]
mod present_mode_tests {
    use crate::host::window_driver::PresentMode::{Direct, SkipCopy, SkipNoop, ViaBackbuffer};
    use crate::host::window_driver::PresentStrategy::{BackbufferCopy, DirectAdaptive};
    use crate::host::window_driver::{PresentMode, present_mode};
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    use crate::ui::frame_report::{RenderKind, RenderPlan};

    /// 100×100 logical surface (10_000 px²) the partial fixtures collapse
    /// against, so a `w×h` damage rect carries `coverage = w·h / 10_000`.
    const SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

    fn full() -> Option<RenderPlan> {
        Some(RenderPlan {
            clear: Color::BLACK,
            kind: RenderKind::Full,
        })
    }
    /// One `Rect` of `w·h` px², built through `collapse_from` against
    /// [`SURFACE`] so its `region.coverage` is `w·h / 10_000` — exactly what the
    /// damage engine seals in the real path.
    fn partial(w: f32, h: f32) -> Option<RenderPlan> {
        let region = DamageRegion::collapse_from(
            &[Rect::new(0.0, 0.0, w, h)],
            DEFAULT_PASS_BUDGET_PX,
            SURFACE,
        );
        Some(RenderPlan {
            clear: Color::BLACK,
            kind: RenderKind::Partial { region },
        })
    }
    const DIRECT_FULL: PresentMode = Direct(RenderPlan {
        clear: Color::BLACK,
        kind: RenderKind::Full,
    });

    #[test]
    fn backbuffer_copy_fills_target_through_backbuffer() {
        // Fresh target each call: paint via the backbuffer (the requested plan,
        // Full or Partial), skip copies it out — the whole target is filled.
        // Backbuffer freshness is irrelevant here (every frame touches it).
        for fresh in [false, true] {
            assert_eq!(
                present_mode(full(), BackbufferCopy, fresh),
                ViaBackbuffer(full().unwrap())
            );
            assert_eq!(
                present_mode(partial(10.0, 10.0), BackbufferCopy, fresh),
                ViaBackbuffer(partial(10.0, 10.0).unwrap())
            );
            assert_eq!(present_mode(None, BackbufferCopy, fresh), SkipCopy);
        }
    }

    #[test]
    fn direct_adaptive_full_and_skip() {
        // A whole-surface repaint goes straight in; a skip is a noop. Neither
        // depends on backbuffer freshness.
        for fresh in [false, true] {
            assert_eq!(
                present_mode(full(), DirectAdaptive, fresh),
                Direct(full().unwrap())
            );
            assert_eq!(present_mode(None, DirectAdaptive, fresh), SkipNoop);
        }
    }

    #[test]
    fn direct_adaptive_small_partial_tracks_backbuffer_freshness() {
        // 10×10 = 100 px² ⇒ coverage 0.01, well under the 0.4 promote line.
        let small = partial(10.0, 10.0);
        // Fresh: the backbuffer mirrors the target, so paint just the region.
        assert_eq!(
            present_mode(small, DirectAdaptive, true),
            ViaBackbuffer(small.unwrap())
        );
        // Stale (after a direct frame): resync with one full repaint first.
        assert_eq!(
            present_mode(small, DirectAdaptive, false),
            ViaBackbuffer(full().unwrap())
        );
    }

    #[test]
    fn direct_adaptive_large_partial_promotes_to_direct() {
        // 80×80 = 6_400 px² ⇒ coverage 0.64 > 0.4: a large partial repaints
        // direct (dropping the copy) regardless of backbuffer freshness.
        let large = partial(80.0, 80.0);
        for fresh in [false, true] {
            assert_eq!(present_mode(large, DirectAdaptive, fresh), DIRECT_FULL);
        }
    }

    #[test]
    fn direct_adaptive_promote_threshold_is_strict() {
        // Coverage at-or-below 0.4 stays on the backbuffer path (`>`, not `>=`);
        // just over promotes. 63×63 = 3_969 (0.3969) vs 64×64 = 4_096 (0.4096) —
        // straddling the 0.4 line.
        assert!(matches!(
            present_mode(partial(63.0, 63.0), DirectAdaptive, true),
            ViaBackbuffer(_)
        ));
        assert_eq!(
            present_mode(partial(64.0, 64.0), DirectAdaptive, true),
            DIRECT_FULL
        );
    }
}

#[cfg(test)]
mod output_validity_tests {
    use crate::host::shared::HostShared;
    use crate::host::window_driver::{PresentMode, PresentStrategy, WindowDriver};
    use crate::primitives::color::Color;
    use crate::text::TextShaper;
    use crate::ui::frame_report::{FrameProcessing, FrameReport, RenderKind, RenderPlan};
    use crate::window::WindowToken;

    fn report(plan: Option<RenderPlan>) -> FrameReport {
        FrameReport {
            repaint_requested: false,
            repaint_after: None,
            plan,
            processing: FrameProcessing::SingleLayout,
        }
    }

    #[test]
    fn output_validity_tracks_invalidation_pending_and_completion() {
        let shared = HostShared::new(TextShaper::default());
        let mut driver = WindowDriver::builder(WindowToken(1), &shared, 8192).build();
        assert!(!driver.output_valid, "first frame has no presented output");

        driver.output_valid = true;
        driver.backbuffer_fresh = true;
        driver.invalidate_target();
        assert!(!driver.output_valid, "target change invalidates output");
        assert!(
            !driver.backbuffer_fresh,
            "target change invalidates retained target pixels"
        );

        driver.output_valid = true;
        let paint = driver.finish_cpu_frame(report(Some(RenderPlan {
            clear: Color::BLACK,
            kind: RenderKind::Full,
        })));
        assert!(matches!(paint.mode, PresentMode::Direct(_)));
        assert!(
            !driver.output_valid,
            "paint stays pending until acquire and submit complete"
        );

        driver.output_valid = true;
        assert!(driver.output_valid, "successful submit restores validity");

        let skip = driver.finish_cpu_frame(report(None));
        assert!(matches!(skip.mode, PresentMode::SkipNoop));
        assert!(
            driver.output_valid,
            "SkipNoop preserves valid target pixels"
        );

        driver.strategy = PresentStrategy::BackbufferCopy;
        let skip_copy = driver.finish_cpu_frame(report(None));
        assert!(matches!(skip_copy.mode, PresentMode::SkipCopy));
        assert!(
            !driver.output_valid,
            "SkipCopy stays pending until the copy is submitted"
        );
        driver.output_valid = true;
        assert!(driver.output_valid, "successful copy restores validity");
    }
}

#[cfg(test)]
mod record_store_tests {
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::app::App;
    use crate::app::test_support::RecordApp;
    use crate::host::clock::FixedClock;
    use crate::host::shared::HostShared;
    use crate::host::window_driver::{PresentStrategy, WindowDriver};
    use crate::primitives::color::{Color, ColorU8};
    use crate::primitives::mesh::{Mesh, MeshVertex};
    use crate::primitives::widget_id::WidgetId;
    use crate::shape::{PolylineColors, Shape};
    use crate::text::TextShaper;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameProcessing;
    use crate::widgets::panel::Panel;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::text::Text;
    use crate::{Configure, Display, WindowToken};

    #[derive(Debug, PartialEq)]
    struct RecordPayloadSnapshot {
        mesh_vertices: Vec<MeshVertex>,
        mesh_indices: Vec<u32>,
        polyline_points: Vec<Vec2>,
        polyline_colors: Vec<ColorU8>,
        text: String,
    }

    #[derive(Debug, Default)]
    struct LifecycleApp {
        updates: Vec<WindowToken>,
        records: Vec<WindowToken>,
    }

    impl App for LifecycleApp {
        fn update(&mut self, win: WindowToken, _ui: &Ui) {
            self.updates.push(win);
        }

        fn record(&mut self, win: WindowToken, _ui: &mut Ui) {
            self.records.push(win);
        }
    }

    fn snapshot(driver: &WindowDriver) -> RecordPayloadSnapshot {
        let payloads = driver.ui.record_store.payloads.borrow();
        RecordPayloadSnapshot {
            mesh_vertices: payloads.meshes.vertices.clone(),
            mesh_indices: payloads.meshes.indices.clone(),
            polyline_points: payloads.polyline_points.clone(),
            polyline_colors: payloads.polyline_colors.clone(),
            text: payloads.text_bytes().to_owned(),
        }
    }

    fn record_scene(
        ui: &mut Ui,
        mesh: &Mesh,
        points: &[Vec2],
        colors: &[Color],
        label: &str,
        id: &'static str,
    ) {
        Panel::zstack()
            .id(WidgetId::from_hash(id))
            .size(96.0)
            .show(ui, |ui| {
                ui.add_shape(Shape::mesh(mesh));
                ui.add_shape(Shape::polyline(
                    points,
                    PolylineColors::PerPoint(colors),
                    3.0,
                ));
                let label = ui.intern(label);
                Text::new(label)
                    .id(WidgetId::from_hash((id, "text")))
                    .show(ui);
                Spinner::new()
                    .id(WidgetId::from_hash((id, "spinner")))
                    .size(92.0)
                    .show(ui);
            });
    }

    #[test]
    fn cpu_frame_forwards_token_through_app_lifecycle() {
        let shared = HostShared::new(TextShaper::default());
        let token = WindowToken(17);
        let mut window = WindowDriver::builder(token, &shared, 8192)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .pixel_snap(false)
            .build();
        assert_eq!(window.strategy, PresentStrategy::DirectAdaptive);
        assert!(!window.pixel_snap);
        assert_eq!(window.clock.now(), Duration::ZERO);
        let mut app = LifecycleApp::default();

        let _ = window.cpu_frame(Display::from_physical(UVec2::new(112, 112), 1.0), &mut app);

        assert_eq!(app.updates, [token], "update runs once");
        assert_eq!(
            app.records,
            [token, token],
            "cold-start warmup and visible pass share the token",
        );
    }

    /// A record pass in one window must not replace the payloads retained by
    /// another window's animation-only frame.
    #[test]
    fn interleaved_window_paint_only_preserves_record_payloads() {
        let shared = HostShared::new(TextShaper::default());
        let mut window_a = WindowDriver::builder(WindowToken(1), &shared, 8192)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let mut window_b = WindowDriver::builder(WindowToken(2), &shared, 8192)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let display = Display::from_physical(UVec2::new(112, 112), 1.0);

        let mesh_a = Mesh::filled_triangle(
            Vec2::new(12.0, 14.0),
            Vec2::new(72.0, 20.0),
            Vec2::new(26.0, 74.0),
            Color::rgb(0.15, 0.65, 0.95),
        );
        let points_a = [
            Vec2::new(8.0, 82.0),
            Vec2::new(28.0, 10.0),
            Vec2::new(68.0, 84.0),
            Vec2::new(88.0, 12.0),
        ];
        let colors_a = [
            Color::rgb(1.0, 0.0, 0.0),
            Color::WHITE,
            Color::rgb(0.0, 1.0, 0.0),
            Color::rgb(0.0, 0.0, 1.0),
        ];

        let mesh_b = Mesh::filled_polygon(
            &[
                Vec2::new(78.0, 8.0),
                Vec2::new(90.0, 46.0),
                Vec2::new(58.0, 88.0),
                Vec2::new(14.0, 70.0),
                Vec2::new(8.0, 24.0),
            ],
            Color::rgb(0.9, 0.2, 0.65),
        );
        let points_b = [
            Vec2::new(90.0, 88.0),
            Vec2::new(82.0, 18.0),
            Vec2::new(58.0, 64.0),
            Vec2::new(38.0, 14.0),
            Vec2::new(20.0, 76.0),
            Vec2::new(6.0, 32.0),
        ];
        let colors_b = [
            Color::WHITE,
            Color::rgb(0.0, 0.0, 1.0),
            Color::rgb(0.0, 1.0, 0.0),
            Color::rgb(1.0, 0.0, 0.0),
            Color::BLACK,
            Color::WHITE,
        ];

        let mut app_a = RecordApp::new(|ui| {
            record_scene(ui, &mesh_a, &points_a, &colors_a, "retained A", "window-a");
        });
        let _ = window_a.cpu_frame(display, &mut app_a);
        window_a.output_valid = true;
        let retained = snapshot(&window_a);
        assert_eq!(retained.mesh_vertices.len(), 3);
        assert_eq!(retained.polyline_points.len(), 4);
        assert_eq!(retained.text, "retained A");

        let mut app_b = RecordApp::new(|ui| {
            record_scene(
                ui,
                &mesh_b,
                &points_b,
                &colors_b,
                "window B has a much longer label",
                "window-b",
            );
        });
        let _ = window_b.cpu_frame(display, &mut app_b);
        window_b.output_valid = true;
        assert_eq!(snapshot(&window_a), retained);

        let paint_only = window_a.cpu_frame(display, &mut app_a);
        assert_eq!(paint_only.report.processing, FrameProcessing::PaintOnly);
        assert_eq!(snapshot(&window_a), retained);
    }
}
