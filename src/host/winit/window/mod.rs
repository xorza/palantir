//! Per-window winit state and swapchain frame orchestration.

mod applied_levels;
mod present_schedule;
mod visibility;

use std::sync::Arc;
use std::time::Instant;

use glam::{IVec2, UVec2, Vec2};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::window::Window as WinitWindow;

use crate::common::platform::PLATFORM;
use crate::common::tracy::{self, FrameSet};
use crate::display::system_scale::SystemScale;
use crate::gpu::surface::window_surface::{Acquired, Prepared, WindowSurface};
use crate::host::clock::RealtimeClock;
use crate::host::core::HostCore;
use crate::host::window_driver::{CpuFrame, FrameSpec, PresentPath, TargetKey, WindowDriver};
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::input::translator::{PointerTrace, Translator};
use crate::host::winit::native;
use crate::host::winit::remote::remote_end::RemoteEnd;
use crate::host::winit::remote::{QueryTask, ShotAsk};
use crate::host::winit::window::applied_levels::{AppliedLevels, ImeChange};
use crate::host::winit::window::present_schedule::{AcquireOutcome, PresentSchedule, Wake};
use crate::host::winit::window::visibility::Visibility;
use crate::host::winit::window_set::Paced;
use crate::input::input_event::InputEvent;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::app::App;
use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;
use crate::window::window_frame_state::WindowFacts;
use crate::window::window_placement::WindowPlacement;
use std::mem;

/// What only the windowing system can answer, held until an event that can change
/// it. Each field is an X11 round trip and `current_monitor` allocates, which
/// per-frame asking paid for readers that ask only on
/// [`Ui::window_geometry`](crate::Ui::window_geometry) and the refresh rate.
/// [`Window::invalidate_system_facts`] names the events that clear it.
#[derive(Clone, Copy, Debug)]
pub(super) struct SystemFacts {
    placement: WindowPlacement,
    /// Whether the window fills its screen, as the macOS green button makes it; like
    /// maximized, a size the window does not return to.
    fullscreen: bool,
    refresh_millihertz: Option<u32>,
}

impl SystemFacts {
    /// Whether the window shows at its own size: neither maximized nor full screen.
    const fn is_normal(self) -> bool {
        !self.placement.maximized && !self.fullscreen
    }
}

/// Where the pointer is, and the scale the recorder was last told it in: one fact,
/// since staleness is noticed by comparing that scale with the current one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PointerAnchor {
    physical: Vec2,
    /// The effective scale reported when this position was last delivered.
    scale: f32,
}

impl PointerAnchor {
    const fn after(held: Option<Self>, trace: PointerTrace, scale: f32) -> Option<Self> {
        match trace {
            PointerTrace::Unchanged => held,
            PointerTrace::At(physical) => Some(Self { physical, scale }),
            PointerTrace::Gone => None,
        }
    }

    /// Adopt `scale` and return the logical position to re-tell the recorder, or `None`
    /// when the scale has not moved.
    fn restate_at(&mut self, scale: f32) -> Option<Vec2> {
        if self.scale == scale {
            return None;
        }
        self.scale = scale;
        Some(self.physical / scale)
    }
}

/// Everything one native window owns.
#[derive(Debug)]
pub(super) struct Window {
    pub(super) window: Arc<WinitWindow>,
    pub(super) surface: WindowSurface,
    pub(super) driver: WindowDriver,
    /// The device pixel ratio winit reports; the drawn scale is [`Self::effective_scale`].
    pub(super) system_scale: SystemScale,
    pub(super) next: FramePresent,
    pub(super) close_requested: bool,
    levels: AppliedLevels,
    visibility: Visibility,
    schedule: PresentSchedule,
    translator: Translator,
    /// The frame clock, which skips the spans the window is hidden.
    clock: RealtimeClock,
    /// Origin of the input clock. Not the frame clock, which skips a hidden span: two presses
    /// either side would read as a double-click.
    input_epoch: Instant,
    /// A resize can change only whether the window is maximized or full screen (moves arrive as
    /// `Moved`), so only those facts are marked stale.
    maximized_stale: bool,
    /// The content size in physical px, as the last resize reported it.
    inner: UVec2,
    /// The content size in physical px the window returns to from maximized or full screen: its
    /// size at the last frame it was neither, or the size it opened with, as Qt's
    /// `saveGeometry` keeps the normal geometry apart from the maximized one.
    restored: Option<UVec2>,
    frame_set: FrameSet,
    system_facts: Option<SystemFacts>,
    pointer: Option<PointerAnchor>,
    /// What the window answers its remotes with.
    remote: RemoteEnd,
}

/// What a [`Window`] opens with: its native half, its surface and driver, its decorations'
/// scheme, and the logical content size its config asked for, which it returns to from
/// maximized until it shows unmaximized.
#[derive(Debug)]
pub(super) struct OpenedWindow {
    pub(super) window: Arc<WinitWindow>,
    pub(super) surface: WindowSurface,
    pub(super) driver: WindowDriver,
    pub(super) decorations: ColorScheme,
    pub(super) inner_size: Option<Size>,
}

impl Window {
    pub(super) fn new(opened: OpenedWindow) -> Self {
        let OpenedWindow {
            window,
            surface,
            mut driver,
            decorations,
            inner_size,
        } = opened;
        let system_scale = SystemScale::from_platform(window.scale_factor());
        let restored = inner_size.map(|size| {
            (Vec2::from(size) * system_scale.get())
                .round()
                .as_uvec2()
                .max(UVec2::ONE)
        });
        // Seed the recorder's pacing level from the opened swapchain, so `Ui::vsync` is
        // truthful before any frame.
        driver.ui.seed_vsync(surface.vsync());
        Self {
            inner: native::physical_size(&window),
            restored,
            window,
            surface,
            driver,
            system_scale,
            next: FramePresent::Immediate,
            close_requested: false,
            levels: AppliedLevels::new(decorations),
            visibility: Visibility::default(),
            schedule: PresentSchedule::default(),
            translator: Translator::new(PLATFORM),
            clock: RealtimeClock::new(),
            input_epoch: Instant::now(),
            maximized_stale: false,
            frame_set: FrameSet::claim(),
            system_facts: None,
            pointer: None,
            remote: RemoteEnd::default(),
        }
    }

    /// Ask the window for the screenshot a remote wants, which a later frame hands back.
    pub(super) fn ask_shot(&mut self, ask: ShotAsk) {
        self.remote.ask_shot(&mut self.driver.ui, ask);
        self.next = FramePresent::Immediate;
    }

    /// Translate `event` into the recorder's input. Returns whether it asks for a repaint.
    pub(super) fn on_window_event(&mut self, event: &WindowEvent) -> bool {
        let scale = self.translation_scale();
        // Stamped where the event arrived: the frame clock stands still between frames.
        let now = self.input_epoch.elapsed();
        let ui = &mut self.driver.ui;
        let mut repaint_requested = false;
        let trace = self.translator.translate(event, scale, |input| {
            repaint_requested |= ui.on_input(input, now).repaint_requested;
        });
        self.note_pointer(trace, scale);
        repaint_requested
    }

    /// Deliver `event`, in logical px, as [`Self::on_window_event`] delivers what it translates,
    /// then ask for a frame, so the sender sees one that ran after it.
    pub(super) fn on_sent_input(&mut self, event: InputEvent<'_>) {
        let scale = self.translation_scale();
        let now = self.input_epoch.elapsed();
        self.driver.ui.on_input(event, now);
        self.note_pointer(PointerTrace::of_input(event, scale), scale);
        self.remote.note_sent();
        self.next = FramePresent::Immediate;
    }

    /// Run `task` with `&mut` `app` and this window's `Ui`, then ask for a frame that records, as
    /// [`Self::on_sent_input`] does for an input.
    pub(super) fn on_query<T>(&mut self, app: &mut T, task: QueryTask<T>) {
        self.remote.run_query(app, &mut self.driver.ui, task);
        self.next = FramePresent::Immediate;
    }

    /// The scale an event's position is divided by: the one the current cascade was
    /// laid out at, which the event is hit-tested against. The live scale would
    /// misplace a click queued before a user-scale write lands; `resync_pointer`
    /// restates the pointer when it moves.
    fn translation_scale(&self) -> f32 {
        self.driver
            .ui
            .laid_out_scale()
            .unwrap_or_else(|| self.effective_scale())
    }

    const fn note_pointer(&mut self, trace: PointerTrace, scale: f32) {
        self.pointer = PointerAnchor::after(self.pointer, trace, scale);
    }

    /// Re-tell the recorder where the pointer is when the effective scale has moved.
    /// The pointer is the one input that outlives its event, so a scale change would
    /// otherwise hit-test the new layout against an old point until the mouse moves.
    fn resync_pointer(&mut self) {
        let scale = self.effective_scale();
        if let Some(anchor) = &mut self.pointer
            && let Some(logical) = anchor.restate_at(scale)
        {
            let now = self.input_epoch.elapsed();
            self.driver
                .ui
                .on_input(InputEvent::PointerMoved(logical), now);
        }
    }

    /// Physical pixels per logical pixel as the app sees them (platform times app),
    /// read live since the user scale is written inside a frame. Events between frames
    /// use [`Self::translation_scale`].
    pub(super) fn effective_scale(&self) -> f32 {
        self.driver.ui.user_scale().applied_to(self.system_scale)
    }

    /// This frame's [`SystemFacts`], asking the windowing system only after an event
    /// invalidated them.
    fn system_facts(&mut self) -> SystemFacts {
        if let Some(facts) = &mut self.system_facts {
            if mem::take(&mut self.maximized_stale) {
                facts.placement.maximized = self.window.is_maximized();
                facts.fullscreen = self.window.fullscreen().is_some();
            }
            return *facts;
        }
        self.maximized_stale = false;
        let facts = SystemFacts {
            placement: WindowPlacement {
                physical_pos: self
                    .window
                    .outer_position()
                    .ok()
                    .map(|pos| IVec2::new(pos.x, pos.y)),
                maximized: self.window.is_maximized(),
            },
            fullscreen: self.window.fullscreen().is_some(),
            refresh_millihertz: self
                .window
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz()),
        };
        self.system_facts = Some(facts);
        facts
    }

    /// Drop the cached [`SystemFacts`]. Called for every event that can move the window,
    /// resize it or change its monitor; a missed event is unsafe, a superset is not.
    pub(super) const fn invalidate_system_facts(&mut self) {
        self.system_facts = None;
    }

    /// The content now `inner` physical px, not zero: whether the window is maximized may have
    /// changed too.
    pub(super) const fn note_resized(&mut self, inner: UVec2) {
        self.inner = inner;
        self.maximized_stale = true;
    }

    /// Whether the window is minimized or occluded, and so skips its frames.
    pub(super) const fn is_hidden(&self) -> bool {
        self.visibility.is_hidden()
    }

    pub(super) fn set_occluded(&mut self, occluded: bool) {
        if let Some(hidden) = self.visibility.set_occluded(occluded, Instant::now()) {
            self.clock.skip(hidden);
        }
    }

    /// The window was resized to zero (minimized on Windows) or back. Returns whether
    /// that changed anything.
    pub(super) fn set_minimized(&mut self, minimized: bool) -> bool {
        let changed = self.visibility.is_minimized() != minimized;
        if let Some(hidden) = self.visibility.set_minimized(minimized, Instant::now()) {
            self.clock.skip(hidden);
        }
        changed
    }

    /// Run one application/UI frame, acquire the swapchain texture, present it, then
    /// drain window-host output into `commands`. Stores the schedule on [`Self::next`].
    /// Returns the frame's summary, or `None` when the hidden window skipped it.
    pub(super) fn frame<T: App>(
        &mut self,
        core: &mut HostCore,
        app: &mut T,
        commands: &mut WindowCommands,
    ) -> Option<RanFrame> {
        tracy::zone!("Window::frame");

        let facts = self.system_facts();
        if facts.is_normal() {
            self.restored = Some(self.inner);
        }
        // Also where the previous frame's veto is asserted spent (`Ui::set_window_facts`).
        // `finish` is outside the occlusion branch, so every frame reaches the drain.
        self.driver.ui.set_window_facts(WindowFacts {
            close_requested: self.close_requested,
            placement: facts.placement,
            restored_size: self.restored,
            shown_again: self.visibility.take_shown_again(),
        });

        // A hidden window skips its frame except the one carrying a close request:
        // only `App::update` / `App::record` can veto, and skipping would close a
        // minimized document past its "save changes?" prompt. One-shot.
        let ran = if self.visibility.is_hidden() && !self.close_requested {
            self.next = FramePresent::Idle;
            None
        } else {
            // The close-request frame runs while hidden, and showing again skips the hidden span
            // assuming no frame ran in it. Restart the span here, or a vetoed close lets the
            // skip move the origin past this frame's stamp and the frame clock goes backwards
            // (`advance_clock` still assigns `time`), skewing repaint deadlines and multi-press
            // timing.
            self.visibility.restart(Instant::now());
            // Before the display is minted, so a frame at a new scale hit-tests in the same space.
            self.resync_pointer();
            // Before the record, which reads whether the target still holds the last frame.
            if self.surface.prepare(core.device.gpu()) == Prepared::Rebuilt {
                self.driver.invalidate_target_contents();
            }
            let spec = FrameSpec {
                target: TargetKey {
                    physical: self.surface.size(),
                    format: self.surface.format(),
                    gives_copy: self.surface.gives_copy(),
                },
                system_scale: self.system_scale,
                refresh_millihertz: facts.refresh_millihertz,
                now: self.clock.now(),
            };
            let cpu = core
                .device
                .record(&mut core.cpu, &mut self.driver, spec, app);
            let summary = self
                .remote
                .after_frame(&self.driver.ui, &cpu.report, app.is_busy());
            let ran_app = cpu.report.processing.ran_app();
            self.next = self.present(core, cpu);
            Some(RanFrame { summary, ran_app })
        };

        self.finish(commands);
        // This window's own frame set; marking the main one (`WinitRuntime::draw` owns it)
        // made Tracy report slices as whole frames. Marked past every exit so an occluded
        // frame closes its own frame.
        self.frame_set.mark();
        ran
    }

    fn present(&mut self, core: &mut HostCore, cpu: CpuFrame) -> FramePresent {
        let CpuFrame { report, mode } = cpu;
        // A lost device paints nothing; its recovery wakes the window again.
        let outcome = if report.plan.is_none() || core.device.gpu().is_lost() {
            AcquireOutcome::Skipped
        } else {
            self.acquire_and_present(core, mode)
        };
        let decision = self.schedule.after(outcome, &report);
        if decision.rebuild {
            if let AcquireOutcome::Suboptimal(_) = outcome {
                tracing::warn!("surface acquire: suboptimal");
            }
            self.surface.mark_stale();
        }
        match decision.wake {
            Wake::Now => FramePresent::Immediate,
            Wake::After(delay) => FramePresent::At(self.clock.deadline(self.clock.now() + delay)),
            Wake::At(due) => FramePresent::At(self.clock.deadline(due)),
            Wake::Idle => FramePresent::Idle,
        }
    }

    /// Acquire the swapchain image and, if it came, draw `mode` into it and present.
    fn acquire_and_present(&mut self, core: &mut HostCore, mode: PresentPath) -> AcquireOutcome {
        match self.surface.acquire() {
            Acquired::Ready(frame) => {
                core.device
                    .submit(&core.cpu, &mut self.driver, frame.target(), mode);
                self.window.pre_present_notify();
                frame.present(core.device.gpu());
                AcquireOutcome::Presented
            }
            Acquired::Suboptimal(frame) => {
                core.device
                    .submit(&core.cpu, &mut self.driver, frame.target(), mode);
                self.window.pre_present_notify();
                frame.present(core.device.gpu());
                AcquireOutcome::Suboptimal(self.surface.size())
            }
            Acquired::Outdated => {
                tracing::warn!("surface acquire: outdated");
                AcquireOutcome::Outdated
            }
            Acquired::Lost => {
                tracing::warn!("surface acquire: lost");
                AcquireOutcome::Lost
            }
            Acquired::Timeout => {
                tracing::warn!("surface acquire: timeout");
                AcquireOutcome::Timeout
            }
            Acquired::Validation => {
                tracing::warn!("surface acquire: validation");
                AcquireOutcome::Validation
            }
            Acquired::Unconfigured => {
                tracing::warn!("surface acquire: no swapchain built");
                AcquireOutcome::Validation
            }
            Acquired::Occluded => AcquireOutcome::Occluded,
        }
    }

    /// Settle what the frame produced: drain the recorder's window commands (an
    /// un-vetoed close request becomes this window's close command), push the cursor,
    /// apply a vsync change, and consume the one-shot close request.
    fn finish(&mut self, commands: &mut WindowCommands) {
        let output = self.driver.drain_window_output(commands);
        if self.levels.set_cursor(output.cursor) {
            self.window.set_cursor(native::cursor(output.cursor));
        }
        self.set_vsync(output.vsync);
        self.set_ime_area(output.ime_area);
        let color_scheme = self.driver.ui.color_scheme();
        if self.levels.set_color_scheme(color_scheme) {
            self.window
                .set_theme(Some(native::color_scheme(color_scheme)));
        }
        self.close_requested = false;
    }

    /// Turn the platform's input method on beside `caret` (logical px, at the scale the frame
    /// laid it out at), or off for `None`; only on a change.
    fn set_ime_area(&mut self, caret: Option<Rect>) {
        let scale = self.translation_scale();
        let physical = caret.map(|rect| physical_rect(rect, scale));
        let area = match self.levels.set_ime_area(physical) {
            None => return,
            Some(ImeChange::Disable) => {
                self.window.set_ime_allowed(false);
                return;
            }
            Some(ImeChange::Enable(rect)) => {
                self.window.set_ime_allowed(true);
                rect
            }
            Some(ImeChange::Move(rect)) => rect,
        };
        self.window.set_ime_cursor_area(
            PhysicalPosition::new(f64::from(area.min.x), f64::from(area.min.y)),
            PhysicalSize::new(f64::from(area.size.w), f64::from(area.size.h)),
        );
    }

    /// Point the swapchain at `vsync`, if not already paced that way; the next frame's
    /// [`WindowSurface::prepare`] rebuilds it. The forced repaint is because an idle
    /// window schedules no next frame.
    fn set_vsync(&mut self, vsync: Vsync) {
        if self.surface.set_vsync(vsync) {
            self.next = FramePresent::Immediate;
        }
    }
}

impl Paced for Window {
    fn next(&self) -> FramePresent {
        self.next
    }

    fn set_next(&mut self, next: FramePresent) {
        self.next = next;
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

/// A frame a window ran: its summary, and whether it ran the app, which the other windows follow.
#[derive(Clone, Copy, Debug)]
pub(super) struct RanFrame {
    pub(super) summary: FrameSummary,
    /// [`FrameProcessing::ran_app`](crate::ui::frame_report::FrameProcessing::ran_app).
    pub(super) ran_app: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum FramePresent {
    Immediate,
    At(Instant),
    Idle,
    /// Due now, only to show what another window's frame changed in the app every window
    /// records from. A frame run for this alone asks no window to follow it, so two windows
    /// cannot keep each other running.
    Follow,
}

impl FramePresent {
    /// Collapse a deadline already due into `Immediate`; a past `WaitUntil` spins the loop.
    pub(super) fn resolve(self, now: Instant) -> Self {
        match self {
            Self::At(t) if t <= now => Self::Immediate,
            other => other,
        }
    }

    /// This window's next frame once another window ran one of its own, which may have changed
    /// the app: due now, as a [`Self::Follow`] unless it was due for itself. A deadline still
    /// ahead folds in, as the follow frame reports it again.
    ///
    /// Every window follows each frame another window runs of its own, as iced rebuilds every
    /// window's view after each update: the host cannot see what a frame changed in the app. Refused: a
    /// repaint the app asks of each window it changes (egui's `request_repaint_of`), or a revision
    /// it bumps (Druid's `Data` diff), which a missed call leaves stale with no sign. Cost: a
    /// record and a layout of each other window a frame, where damage paints what changed.
    pub(super) fn followed(self, now: Instant) -> Self {
        match self.resolve(now) {
            Self::Immediate => Self::Immediate,
            Self::At(_) | Self::Idle | Self::Follow => Self::Follow,
        }
    }

    /// Whether a frame due as this is the window's own, which the other windows follow when it
    /// ran the app (`ran_app`): a paint-only frame, a caret's blink or a spinner's turn, ran none
    /// of it, so it changed nothing another window shows.
    pub(super) fn leads(self, now: Instant, ran_app: bool) -> bool {
        ran_app && self.resolve(now) != Self::Follow
    }
}

/// `rect` in logical px as physical px at `scale`.
fn physical_rect(rect: Rect, scale: f32) -> Rect {
    Rect {
        min: rect.min * scale,
        size: rect.size.scaled_by(scale),
    }
}

#[cfg(test)]
mod tests;
