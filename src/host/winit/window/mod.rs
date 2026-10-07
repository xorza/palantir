//! Per-window winit state and swapchain frame orchestration.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{IVec2, UVec2, Vec2};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::keyboard::ModifiersState;
use winit::window::Window as WinitWindow;

use crate::app::App;
use crate::common::tracy::{self, FrameSet};
use crate::display;
use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::surface::window_surface::{Acquired, WindowSurface};
use crate::host::core::HostCore;
use crate::host::window_driver::{CpuFrame, TargetKey, WindowDriver};
use crate::host::winit::input::PointerTrace;
use crate::host::winit::native;
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::primitives::geometry::rect::Rect;
use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;
use crate::window::window_frame_state::WindowFrameState;
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
    refresh_millihertz: Option<u32>,
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

/// First delay after a `Validation` acquire: about one frame at 60 Hz.
const ACQUIRE_RETRY_MIN: Duration = Duration::from_millis(16);

/// Ceiling for [`Window::acquire_retry`]: two wake-ups a second.
const ACQUIRE_RETRY_MAX: Duration = Duration::from_millis(500);

/// Everything one native window owns.
#[derive(Debug)]
pub(super) struct Window {
    pub(super) window: Arc<WinitWindow>,
    pub(super) surface: WindowSurface,
    pub(super) driver: WindowDriver,
    /// The device pixel ratio winit reports; the drawn scale is [`Self::effective_scale`].
    pub(super) system_scale: f32,
    pub(super) next: FramePresent,
    pub(super) close_requested: bool,
    cursor: CursorIcon,
    /// The IME caret area in force, in physical px, or `None` while IME is off. Physical
    /// so a scale change re-places the candidate list.
    ime_area: Option<Rect>,
    /// When the window became hidden; its clock skips the gap on resume.
    occluded_at: Option<Instant>,
    /// The platform reported the window occluded; [`Self::minimized`] is the other
    /// hidden reason.
    occluded: bool,
    /// Resized to zero: how Windows reports a minimize (no `Occluded`).
    minimized: bool,
    pub(super) modifiers: ModifiersState,
    /// Origin of the input clock. Not the frame clock, which `Clock::skip` rewinds over
    /// a hidden span: two presses either side would read as a double-click.
    input_epoch: Instant,
    /// A resize can change only whether the window is maximized (moves arrive as
    /// `Moved`), so only that fact is marked stale.
    maximized_stale: bool,
    /// The surface size a suboptimal acquire last reconfigured for: the swapchain
    /// rebuilds once per size, not every frame while a resize is pending.
    suboptimal_handled: Option<UVec2>,
    frame_set: FrameSet,
    system_facts: Option<SystemFacts>,
    pointer: Option<PointerAnchor>,
    /// How long the next frame waits before retrying an acquire that failed validation;
    /// `None` while healthy. Other failures are transient and an immediate repaint
    /// settles them, but a validation failure repeats, so without a delay the host
    /// builds a full CPU draw list per loop iteration for nothing. Doubles to
    /// [`ACQUIRE_RETRY_MAX`].
    acquire_retry: Option<Duration>,
}

impl Window {
    pub(super) fn new(
        window: Arc<WinitWindow>,
        surface: WindowSurface,
        mut driver: WindowDriver,
    ) -> Self {
        let system_scale = display::sanitize_system_scale(window.scale_factor());
        // Seed the recorder's pacing level from the opened swapchain, so `Ui::vsync` is
        // truthful before any frame.
        driver.ui.seed_vsync(surface.vsync());
        Self {
            window,
            surface,
            driver,
            system_scale,
            next: FramePresent::Immediate,
            close_requested: false,
            cursor: CursorIcon::default(),
            ime_area: None,
            occluded_at: None,
            occluded: false,
            minimized: false,
            modifiers: ModifiersState::empty(),
            input_epoch: Instant::now(),
            maximized_stale: false,
            suboptimal_handled: None,
            frame_set: FrameSet::claim(),
            system_facts: None,
            pointer: None,
            acquire_retry: None,
        }
    }

    pub(super) fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        // Stamped where the event arrived: the frame clock stands still between frames.
        let now = self.input_epoch.elapsed();
        self.driver.ui.on_input(event, now)
    }

    /// The scale an event's position is divided by: the one the current cascade was
    /// laid out at, which the event is hit-tested against. The live scale would
    /// misplace a click queued before a user-scale write lands; `resync_pointer`
    /// restates the pointer when it moves.
    pub(super) fn translation_scale(&self) -> f32 {
        self.driver
            .ui
            .laid_out_scale()
            .unwrap_or_else(|| self.effective_scale())
    }

    pub(super) const fn note_pointer(&mut self, trace: PointerTrace, scale: f32) {
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
            }
            return *facts;
        }
        self.maximized_stale = false;
        let facts = SystemFacts {
            placement: WindowPlacement {
                position: self
                    .window
                    .outer_position()
                    .ok()
                    .map(|position| IVec2::new(position.x, position.y)),
                maximized: self.window.is_maximized(),
            },
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

    pub(super) const fn note_resized(&mut self) {
        self.maximized_stale = true;
    }

    pub(super) fn set_occluded(&mut self, occluded: bool) {
        self.occluded = occluded;
        self.update_hidden();
    }

    /// The window was resized to zero (minimized on Windows) or back. Returns whether
    /// that changed anything.
    pub(super) fn set_minimized(&mut self, minimized: bool) -> bool {
        let changed = self.minimized != minimized;
        self.minimized = minimized;
        self.update_hidden();
        changed
    }

    fn update_hidden(&mut self) {
        let hidden = self.occluded || self.minimized;
        match (hidden, self.occluded_at) {
            (true, None) => self.occluded_at = Some(Instant::now()),
            (false, Some(at)) => {
                self.occluded_at = None;
                self.driver.clock.skip(at.elapsed());
            }
            _ => {}
        }
    }

    /// Run one application/UI frame, acquire the swapchain texture, present it, then
    /// drain window-host output into `commands`. Stores the schedule on [`Self::next`].
    pub(super) fn frame<T: App>(
        &mut self,
        gpu: &Gpu,
        core: &mut HostCore,
        app: &mut T,
        commands: &mut WindowCommands,
    ) {
        tracy::zone!("Window::frame");

        let facts = self.system_facts();
        // Also where the previous frame's veto is asserted spent (`Ui::set_window_facts`).
        // `finish` is outside the occlusion branch, so every frame reaches the drain.
        self.driver.ui.set_window_facts(WindowFrameState {
            close_requested: self.close_requested,
            placement: facts.placement,
        });

        // An occluded window skips its frame except the one carrying a close request:
        // only `App::update` / `App::record` can veto, and skipping would close a
        // minimized document past its "save changes?" prompt. One-shot.
        if self.occluded_at.is_some() && !self.close_requested {
            self.next = FramePresent::Idle;
        } else {
            // The close-request frame runs while occluded, and `set_occluded(false)` skips the
            // hidden span assuming none did. Restart the span here, or a vetoed close lets the
            // un-occlude move the origin past this frame's stamp and `Clock::now` goes
            // backwards (`advance_clock` still assigns `time`), skewing repaint deadlines and
            // multi-press timing.
            if self.occluded_at.is_some() {
                self.occluded_at = Some(Instant::now());
            }
            // Before the display is minted, so a frame at a new scale hit-tests in the same space.
            self.resync_pointer();
            let physical = self.surface.size();
            let display =
                self.driver
                    .display(physical, self.system_scale, facts.refresh_millihertz);

            // A size, format or present-mode change invalidates retained target state and
            // needs a reconfigure. Identical repeats (Wayland resends configures) must cost
            // nothing: `surface.configure` waits for GPU idle (wgpu #7447: 100ms+ stalls).
            if self.driver.note_target(TargetKey {
                physical,
                format: self.surface.format(),
                vsync: Some(self.surface.vsync()),
            }) {
                self.surface.configure(gpu);
            }

            let cpu = core.cpu_frame(&mut self.driver, display, app);
            self.next = self.present(gpu, core, cpu);
        }

        self.finish(commands);
        // This window's own frame set; marking the main one (`WinitRuntime::draw` owns it)
        // made Tracy report slices as whole frames. Marked past every exit so an occluded
        // frame closes its own frame.
        self.frame_set.mark();
    }

    /// Rebuild the swapchain and tell the driver what it retained went with it; see
    /// [`WindowDriver::invalidate_target_contents`].
    fn reconfigure(&mut self, gpu: &Gpu) {
        self.surface.configure(gpu);
        self.driver.invalidate_target_contents();
    }

    fn present(&mut self, gpu: &Gpu, core: &mut HostCore, cpu: CpuFrame) -> FramePresent {
        let CpuFrame { report, mode } = cpu;
        let repaint = if report.plan.is_none() {
            self.acquire_retry = None;
            report.repaint_requested
        } else {
            let retry = self.acquire_retry.take();
            match self.surface.acquire() {
                Acquired::Ready(frame) => {
                    core.submit(&mut self.driver, frame.target(), mode);
                    self.window.pre_present_notify();
                    frame.present(gpu);
                    report.repaint_requested
                }
                // Still presentable: present it, then rebuild once per size.
                Acquired::Suboptimal(frame) => {
                    core.submit(&mut self.driver, frame.target(), mode);
                    self.window.pre_present_notify();
                    frame.present(gpu);
                    let size = self.surface.size();
                    if self.suboptimal_handled != Some(size) {
                        tracing::warn!("surface acquire: suboptimal");
                        self.suboptimal_handled = Some(size);
                        self.reconfigure(gpu);
                    }
                    report.repaint_requested
                }
                Acquired::Outdated => {
                    tracing::warn!("surface acquire: outdated / lost");
                    self.reconfigure(gpu);
                    true
                }
                Acquired::Timeout => {
                    tracing::warn!("surface acquire: timeout");
                    true
                }
                Acquired::Validation => {
                    tracing::warn!("surface acquire: validation");
                    self.acquire_retry = Some(retry.map_or(ACQUIRE_RETRY_MIN, |delay| {
                        (delay * 2).min(ACQUIRE_RETRY_MAX)
                    }));
                    true
                }
                Acquired::Occluded => false,
            }
        };

        // Ahead of `repaint`, which every failing acquire asks for: the delay paces it.
        if let Some(delay) = self.acquire_retry
            && let Some(at) = self.driver.clock.deadline(self.driver.clock.now() + delay)
        {
            return FramePresent::At(at);
        }
        if repaint {
            FramePresent::Immediate
        } else if let Some(at) = report
            .repaint_after
            .and_then(|duration| self.driver.clock.deadline(duration))
        {
            FramePresent::At(at)
        } else {
            FramePresent::Idle
        }
    }

    /// Settle what the frame produced: drain the recorder's window commands (an
    /// un-vetoed close request becomes this window's close command), push the cursor,
    /// apply a vsync change, and consume the one-shot close request.
    fn finish(&mut self, commands: &mut WindowCommands) {
        let output = self.driver.drain_window_output(commands);
        if output.cursor != self.cursor {
            self.window.set_cursor(native::cursor(output.cursor));
            self.cursor = output.cursor;
        }
        self.set_vsync(output.vsync);
        self.set_ime_area(output.ime);
        self.close_requested = false;
    }

    /// Turn the platform's input method on beside `caret` (logical px), or off for
    /// `None`; only on a change.
    fn set_ime_area(&mut self, caret: Option<Rect>) {
        let physical = caret.map(|rect| physical_rect(rect, self.effective_scale()));
        if physical == self.ime_area {
            return;
        }
        match physical {
            Some(rect) => {
                if self.ime_area.is_none() {
                    self.window.set_ime_allowed(true);
                }
                self.window.set_ime_cursor_area(
                    PhysicalPosition::new(f64::from(rect.min.x), f64::from(rect.min.y)),
                    PhysicalSize::new(f64::from(rect.size.w), f64::from(rect.size.h)),
                );
            }
            None => self.window.set_ime_allowed(false),
        }
        self.ime_area = physical;
    }

    /// Point the swapchain config at `vsync`, if not already paced that way. The
    /// reconfigure is left to the next frame's [`TargetKey`] check: doing it here would
    /// leave `target` naming the old configuration so the check would skip it. The
    /// forced repaint is because an idle window schedules no next frame.
    fn set_vsync(&mut self, vsync: Vsync) {
        if self.surface.set_vsync(vsync) {
            self.next = FramePresent::Immediate;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum FramePresent {
    Immediate,
    At(Instant),
    Idle,
}

impl FramePresent {
    /// Collapse a deadline already due into `Immediate`; a past `WaitUntil` spins the loop.
    pub(super) fn resolve(self, now: Instant) -> Self {
        match self {
            Self::At(t) if t <= now => Self::Immediate,
            other => other,
        }
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
