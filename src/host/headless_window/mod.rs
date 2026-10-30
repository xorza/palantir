//! [`HeadlessWindow`]: one window an app runs in with no desktop, for a harness that should not
//! have to name wgpu.

use crate::app::App;
use crate::display;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::surface::render_target::TargetFormat;
use crate::host::offscreen::{OffscreenHost, OffscreenHostBuilder, OffscreenTime};
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;
use glam::UVec2;
use std::time::Duration;

const LABEL: &str = "palantir.headless_window";

/// An [`OffscreenHost`] and the one texture it renders into, of the window's size: what an app
/// needs to run, take input and paint where no display exists. The texture is replaced only on a
/// resize, which the host sees as a new target, so a still frame repaints nothing.
#[derive(Debug)]
pub struct HeadlessWindow {
    host: OffscreenHost,
    target: OwnedTarget,
    system_scale: f32,
}

/// The settings a [`HeadlessWindow`] opens with: [`OffscreenHostBuilder`]'s, less
/// `retained_target`, which one texture always keeps, plus the target's format.
#[derive(Debug)]
#[must_use]
pub struct HeadlessWindowBuilder {
    gpu: Gpu,
    host: OffscreenHostBuilder,
    format: TargetFormat,
}

impl HeadlessWindowBuilder {
    /// See [`OffscreenHostBuilder::fonts`].
    pub fn fonts(mut self, scope: FontScope) -> Self {
        self.host = self.host.fonts(scope);
        self
    }

    /// See [`OffscreenHostBuilder::shaper`].
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.host = self.host.shaper(shaper);
        self
    }

    /// See [`OffscreenHostBuilder::collect_gpu_stats`].
    pub fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.host = self.host.collect_gpu_stats(collect);
        self
    }

    /// See [`OffscreenHostBuilder::time`].
    pub fn time(mut self, time: OffscreenTime) -> Self {
        self.host = self.host.time(time);
        self
    }

    /// See [`OffscreenHostBuilder::pixel_snap`].
    pub fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.host = self.host.pixel_snap(pixel_snap);
        self
    }

    /// See [`OffscreenHostBuilder::system_clipboard`].
    #[cfg(feature = "system-clipboard")]
    pub fn system_clipboard(mut self, system: bool) -> Self {
        self.host = self.host.system_clipboard(system);
        self
    }

    /// The window texture's format; 8-bit sRGB RGBA by default.
    pub const fn format(mut self, format: TargetFormat) -> Self {
        self.format = format;
        self
    }

    /// Open the window at `size` physical pixels and `system_scale`.
    ///
    /// # Panics
    ///
    /// Panics if `system_scale` is non-finite or below `1e-4`, if either side of `size` is zero
    /// or past the device's largest 2D texture, or as [`OffscreenHostBuilder::build`] does.
    pub fn open(self, size: UVec2, system_scale: f32) -> HeadlessWindow {
        let system_scale = display::scale_factor(system_scale);
        let target = OwnedTarget::window(&self.gpu, LABEL, size, self.format);
        HeadlessWindow {
            host: self.host.build(),
            target,
            system_scale,
        }
    }
}

impl HeadlessWindow {
    /// Start building a window on `gpu`.
    pub fn builder(gpu: Gpu) -> HeadlessWindowBuilder {
        HeadlessWindowBuilder {
            host: OffscreenHost::builder(gpu.clone()).retained_target(true),
            gpu,
            format: TargetFormat::RGBA8_SRGB,
        }
    }

    /// Run one frame of `app` into the window.
    pub fn frame<T: App>(&mut self, app: &mut T) -> FrameReport {
        self.host
            .frame(self.target.as_target(), self.system_scale, app)
    }

    /// See [`OffscreenHost::on_input`].
    pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        self.host.on_input(event)
    }

    /// See [`OffscreenHost::advance`].
    ///
    /// # Panics
    ///
    /// Panics unless the window was built with [`OffscreenTime::Manual`].
    #[track_caller]
    pub fn advance(&mut self, dt: Duration) -> &mut Self {
        self.host.advance(dt);
        self
    }

    /// See [`OffscreenHost::at`].
    ///
    /// # Panics
    ///
    /// Panics unless the window was built with [`OffscreenTime::Manual`].
    #[track_caller]
    pub fn at(&mut self, time: Duration) -> &mut Self {
        self.host.at(time);
        self
    }

    /// See [`OffscreenHost::wait_for_gpu`].
    pub fn wait_for_gpu(&self) {
        self.host.wait_for_gpu();
    }

    /// See [`OffscreenHost::is_device_lost`].
    pub fn is_device_lost(&self) -> bool {
        self.host.is_device_lost()
    }

    /// Draw with `gpu` from now on, as [`OffscreenHost::recover`] does, into a new texture on it,
    /// of the window's size held inside `gpu`'s largest 2D texture.
    ///
    /// # Panics
    ///
    /// As [`OffscreenHost::recover`].
    pub fn recover(&mut self, gpu: Gpu) {
        let size = self
            .target
            .size()
            .min(UVec2::splat(gpu.max_texture_dim().get()));
        self.target = OwnedTarget::window(&gpu, LABEL, size, self.target.as_target().format());
        self.host.recover(gpu);
    }

    /// The window's `Ui`.
    pub const fn ui(&mut self) -> &mut Ui {
        self.host.ui()
    }

    /// The host the window runs on, for its statistics.
    pub const fn host(&self) -> &OffscreenHost {
        &self.host
    }

    /// Give the window `size` physical pixels at `system_scale`; a new size paints in full.
    ///
    /// # Panics
    ///
    /// Panics if `system_scale` is non-finite or below `1e-4`, or if either side of `size` is
    /// zero or past the device's largest 2D texture.
    pub fn resize(&mut self, size: UVec2, system_scale: f32) {
        self.system_scale = display::scale_factor(system_scale);
        if self.target.size() != size {
            self.target = self.target.resized(LABEL, size);
        }
    }

    /// The window's size in physical pixels.
    pub fn size(&self) -> UVec2 {
        self.target.size()
    }

    /// The scale the platform would report for the window.
    pub const fn system_scale(&self) -> f32 {
        self.system_scale
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::host::headless_window::{HeadlessWindow, HeadlessWindowBuilder};
    use image::RgbaImage;

    impl HeadlessWindowBuilder {
        /// See `OffscreenHostBuilder::swapchain_present`.
        pub(crate) fn swapchain_present(mut self) -> Self {
            self.host = self.host.swapchain_present();
            self
        }
    }

    impl HeadlessWindow {
        /// The window's pixels as the last frame left them, waited for. See
        /// [`OwnedTarget::read_back`](crate::internals::OwnedTarget::read_back).
        pub fn read_back(&self) -> RgbaImage {
            self.target.read_back()
        }
    }
}

#[cfg(test)]
mod tests;
