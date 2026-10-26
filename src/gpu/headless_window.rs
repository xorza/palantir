//! [`HeadlessWindow`]: one window an app runs in with no desktop, for a harness outside the crate
//! that should not have to name wgpu.

use glam::UVec2;

use crate::app::App;
use crate::display;
use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::device::requested_gpu::RequestedGpu;
use crate::gpu::error::GpuRequestError;
use crate::gpu::surface::render_target::internals::TargetSpec;
use crate::gpu::surface::render_target::{self, RenderTarget};
use crate::host::clock::Clock;
use crate::host::offscreen::OffscreenHost;
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;

/// An [`OffscreenHost`] on a headless device of its own, rendering into an `Rgba8UnormSrgb`
/// texture of the window's size: what an app needs to run, take input and paint where no display
/// exists. Bundled fonts, so text measures the same on every machine.
#[derive(Debug)]
pub struct HeadlessWindow {
    host: OffscreenHost,
    device: wgpu::Device,
    target: wgpu::Texture,
    scale: f32,
}

impl HeadlessWindow {
    /// A window of `size` physical pixels at `scale`, on a newly opened headless device, timed by
    /// `clock`.
    ///
    /// # Panics
    ///
    /// Panics if `scale` is non-finite or below `1e-4`, as [`OffscreenHost::frame`] does.
    pub fn new(
        size: UVec2,
        scale: f32,
        clock: impl Clock + 'static,
    ) -> Result<Self, GpuRequestError> {
        let requested =
            RequestedGpu::headless(PowerPreference::HighPerformance, wgpu::Features::empty())?;
        let device = requested.gpu.device.clone();
        let host = OffscreenHost::builder(requested.gpu).clock(clock).build();
        let target = target(&device, size);
        let mut window = Self {
            host,
            device,
            target,
            scale: 1.0,
        };
        window.resize(size, scale);
        Ok(window)
    }

    /// Run one frame of `app` into the window.
    pub fn frame<T: App>(&mut self, app: &mut T) -> FrameReport {
        self.host
            .frame(RenderTarget::new(&self.target), self.scale, app)
    }

    /// Deliver one input event, stamped with the window's clock.
    pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        self.host.on_input(event)
    }

    /// The window's `Ui`.
    pub const fn ui(&mut self) -> &mut Ui {
        self.host.ui()
    }

    /// Give the window `size` physical pixels at `scale`; the next frame paints in full.
    ///
    /// # Panics
    ///
    /// Panics if `scale` is non-finite or below `1e-4`.
    pub fn resize(&mut self, size: UVec2, scale: f32) {
        assert!(
            display::scale_factor_is_valid(scale),
            "{}, got {scale}",
            display::SCALE_RULE
        );
        let current = self.target.size();
        if UVec2::new(current.width, current.height) != size {
            self.target = target(&self.device, size);
        }
        self.scale = scale;
    }

    /// The window's size in physical pixels.
    pub fn size(&self) -> UVec2 {
        let size = self.target.size();
        UVec2::new(size.width, size.height)
    }

    /// The scale the window paints at.
    pub const fn scale(&self) -> f32 {
        self.scale
    }
}

fn target(device: &wgpu::Device, size: UVec2) -> wgpu::Texture {
    render_target::internals::texture(
        device,
        TargetSpec {
            label: "palantir.headless_window",
            size,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        },
    )
}
