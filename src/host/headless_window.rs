//! [`HeadlessWindow`]: the one window a headless host runs, which [`OffscreenHost`] and
//! [`Harness`] both compose.
//!
//! [`OffscreenHost`]: crate::OffscreenHost
//! [`Harness`]: crate::Harness

use crate::display::system_scale::SystemScale;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::render_target::RenderTarget;
use crate::host::cpu_core::CpuCore;
use crate::host::device_core::DeviceCore;
use crate::host::host_options::HostOptions;
use crate::host::system_preferences::SystemPreferences;
use crate::host::wake_signal::WakeSignal;
use crate::host::window_driver::{CpuFrame, FrameSpec, TargetKey, WindowDriver};
use crate::text::font_scope::FontScope;
use crate::ui::app::App;
use crate::ui::frame_report::FrameReport;
use crate::window::window_commands::{WindowCommand, WindowCommands};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::vec::Drain;

/// A headless host's one window: the CPU core, the window's driver, the wakes its waker sends, the
/// display facts the host reports, and the window commands its last frame recorded. Each frame
/// runs through [`Self::record_frame`], [`Self::compose_frame`] or [`Self::paint_frame`], so the
/// two headless hosts keep only their clock and their target. Refused: a field and a method of
/// each copied into both hosts, which drifted (the window commands, the system's preferences).
#[derive(Debug)]
pub(super) struct HeadlessWindow {
    pub(super) cpu: CpuCore,
    pub(super) driver: WindowDriver,
    wake: Arc<WakeSignal>,
    pub(super) system_scale: SystemScale,
    pub(super) refresh_millihertz: Option<u32>,
    /// What the host reports of the system's settings, which [`Self::set_preferences`] writes.
    preferences: SystemPreferences,
    /// The window commands the last frame recorded.
    commands: WindowCommands,
}

/// What a headless host's builder seals besides its device and its own settings, which both
/// headless builders hold alike.
#[derive(Debug)]
pub(super) struct HeadlessSettings {
    pub(super) options: HostOptions,
    pub(super) system_scale: SystemScale,
    pub(super) refresh_millihertz: Option<u32>,
    pub(super) preferences: SystemPreferences,
}

impl HeadlessSettings {
    /// Shaping against `fonts`, at scale 1, with no refresh rate and no system preference.
    pub(super) const fn new(fonts: FontScope) -> Self {
        Self {
            options: HostOptions::new(fonts),
            system_scale: SystemScale::ONE,
            refresh_millihertz: None,
            preferences: SystemPreferences::NONE,
        }
    }
}

/// What a [`HeadlessWindow`]'s painted frame paints with and on: its frame is laid out for this
/// texture's key, so no frame lays out for a texture other than the one it paints.
#[derive(Debug)]
pub(super) struct HeadlessCanvas<'a> {
    pub(super) device: &'a mut DeviceCore,
    pub(super) target: RenderTarget<'a>,
}

impl HeadlessWindow {
    /// The window `driver` opened on `cpu`, under `settings`' display facts and system
    /// preferences.
    pub(super) fn open(
        cpu: CpuCore,
        mut driver: WindowDriver,
        settings: &HeadlessSettings,
    ) -> Self {
        let wake = Arc::<WakeSignal>::default();
        driver.frames.set_waker(wake.waker());
        let _ = settings.preferences.apply(&cpu.resources);
        Self {
            cpu,
            driver,
            wake,
            system_scale: settings.system_scale,
            refresh_millihertz: settings.refresh_millihertz,
            preferences: settings.preferences,
            commands: WindowCommands::default(),
        }
    }

    /// One frame of `app` at `now` that records through damage and builds no draw list, laid out
    /// for `target`.
    pub(super) fn record_frame(
        &mut self,
        app: &mut impl App,
        target: TargetKey,
        now: Duration,
    ) -> FrameReport {
        self.take_wake();
        let report = self.driver.record(self.spec(target, now), app);
        self.driver.finish_unpainted();
        record_window_commands(&mut self.driver, &mut self.commands);
        report
    }

    /// [`Self::record_frame`] that also builds the draw list, on the CPU, and paints nothing.
    pub(super) fn compose_frame(
        &mut self,
        app: &mut impl App,
        target: TargetKey,
        now: Duration,
    ) -> FrameReport {
        let report = self.record_frame(app, target, now);
        if let Some(plan) = report.plan {
            self.driver.compose(&mut self.cpu.frontend, plan);
        }
        report
    }

    /// One frame of `app` at `now` that paints on `canvas` while its device lives.
    pub(super) fn paint_frame(
        &mut self,
        app: &mut impl App,
        canvas: HeadlessCanvas<'_>,
        now: Duration,
    ) -> FrameReport {
        let HeadlessCanvas { device, target } = canvas;
        self.take_wake();
        let spec = self.spec(TargetKey::of(target), now);
        let CpuFrame { report, mode } = device.frame(&mut self.cpu, &mut self.driver, spec, app);
        if !device.gpu().is_lost() {
            device.submit(&mut self.cpu, &mut self.driver, target, mode);
        }
        record_window_commands(&mut self.driver, &mut self.commands);
        report
    }

    /// A frame opens by taking the waker's wake.
    fn take_wake(&mut self) {
        if self.wake.take() {
            self.driver.frames.ui.request_repaint();
        }
    }

    /// The frame `target` describes at `now`, at the scale and refresh rate the host reports.
    pub(super) const fn spec(&self, target: TargetKey, now: Duration) -> FrameSpec {
        FrameSpec {
            target,
            system_scale: self.system_scale,
            refresh_millihertz: self.refresh_millihertz,
            now,
        }
    }

    /// Whether a [`Waker`](crate::Waker) of the window woke it since the last frame, which the
    /// next frame takes.
    pub(super) fn is_wake_pending(&self) -> bool {
        self.wake.is_pending()
    }

    /// Block until a [`Waker`](crate::Waker) of the window wakes it, or `deadline` passes, in real
    /// time; whether a wake came.
    pub(super) fn wait_for_wake(&self, deadline: Instant) -> bool {
        self.wake.wait(deadline)
    }

    /// The windows the last frame opened or closed, in the order called, which a headless host
    /// applies none of.
    pub(super) const fn commands(&self) -> &WindowCommands {
        &self.commands
    }

    /// Take the window commands the last frame recorded, for a host that keeps every frame's.
    pub(super) fn drain_commands(&mut self) -> Drain<'_, WindowCommand> {
        self.commands.drain()
    }

    /// Report the system's settings as `change` leaves them; a change asks the window for a
    /// frame.
    pub(super) fn set_preferences(&mut self, change: impl FnOnce(&mut SystemPreferences)) {
        change(&mut self.preferences);
        if self.preferences.apply(&self.cpu.resources) {
            self.driver.frames.ui.request_repaint();
        }
    }

    /// Paint with `gpu` from now on, in place of `device`'s lost one: every GPU object is built
    /// again on it, and the UI state stays.
    pub(super) fn recover(&mut self, device: &mut DeviceCore, gpu: Gpu) {
        device.attach(gpu, &mut self.cpu, [&mut self.driver]);
    }
}

/// Drain a frame's window output into `commands` on a host with no window lifecycle, which keeps
/// one frame's: levels (cursor, vsync) stay with the recorder and are inert here, and the opens
/// and closes are kept for the app to read.
pub(super) fn record_window_commands(driver: &mut WindowDriver, commands: &mut WindowCommands) {
    commands.clear();
    driver.frames.drain_window_output(commands);
}
