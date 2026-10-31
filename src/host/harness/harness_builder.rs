//! [`HarnessBuilder`]: the settings a [`Harness`] seals at build, and the [`Depth`] its frames
//! run to.

use crate::display;
use crate::display::user_scale::UserScale;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::surface::render_target::TargetFormat;
use crate::host::core::HostCore;
use crate::host::harness::{Harness, Seal, WINDOW};
use crate::host::host_options::HostOptions;
use crate::host::offscreen;
use crate::host::window_driver::{FrameDepth, PresentStrategy};
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::window::window_token::WindowToken;
use glam::UVec2;
use std::time::Duration;

/// The label of the texture a [`Depth::Gpu`] harness paints into.
pub(super) const TARGET_LABEL: &str = "palantir.harness";

/// How far a [`Harness`]'s frames run.
#[derive(Debug)]
pub enum Depth {
    /// Record through damage, and build no draw list: input, layout and hit tests, as a unit
    /// test reads them, at the least cost.
    Record,
    /// Build the draw list too, on the CPU, and paint nothing.
    Compose,
    /// Paint on this device into a texture of the window's size, which a shot reads.
    Gpu(Gpu),
}

/// The settings a [`Harness`] opens with. [`Harness::builder`] sets the defaults each setter
/// names.
#[derive(Debug)]
#[must_use]
pub struct HarnessBuilder {
    size: UVec2,
    scale: f32,
    user_scale: UserScale,
    refresh_millihertz: Option<u32>,
    options: HostOptions,
    depth: Depth,
    strategy: PresentStrategy,
    frame_dt: Duration,
    settle_frames: u32,
    background_wait: Duration,
    format: TargetFormat,
}

impl HarnessBuilder {
    pub(super) const fn new(size: UVec2) -> Self {
        Self {
            size,
            scale: 1.0,
            user_scale: UserScale::ONE,
            refresh_millihertz: None,
            options: HostOptions {
                fonts: FontScope::Bundled,
                shaper: None,
                collect_gpu_stats: false,
                pixel_snap: true,
                #[cfg(feature = "system-clipboard")]
                system_clipboard: false,
            },
            depth: Depth::Record,
            strategy: PresentStrategy::Retained,
            frame_dt: Duration::from_nanos(16_666_667),
            settle_frames: 600,
            background_wait: Duration::from_secs(30),
            format: TargetFormat::RGBA8_SRGB,
        }
    }

    /// Physical pixels per logical pixel, as the platform would report it; 1 by default. The
    /// window stays `size` physical pixels, so at 2 a 600 × 200 window is 300 × 100 logical.
    ///
    /// # Panics
    ///
    /// At [`Self::build`], when `scale` is not finite or below `1e-4`.
    pub const fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// The app's own scale on top of [`Self::scale`], as [`Ui::set_user_scale`](crate::Ui::set_user_scale)
    /// sets it; 1 by default.
    pub const fn user_scale(mut self, scale: UserScale) -> Self {
        self.user_scale = scale;
        self
    }

    /// The monitor's refresh rate, which repaint wakes coalesce to; none by default.
    pub const fn refresh_millihertz(mut self, millihertz: u32) -> Self {
        self.refresh_millihertz = Some(millihertz);
        self
    }

    /// Whether axis-aligned paint edges snap to physical pixels; on by default.
    pub const fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.options.pixel_snap = pixel_snap;
        self
    }

    /// Which faces the window shapes against; [`FontScope::Bundled`] by default, so metrics do
    /// not depend on the machine. A shaper set by [`Self::shaper`] wins over it.
    pub const fn fonts(mut self, scope: FontScope) -> Self {
        self.options.fonts = scope;
        self
    }

    /// Shape through `shaper` instead of one loaded from [`Self::fonts`].
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.options.shaper = Some(shaper);
        self
    }

    /// How far each frame runs; [`Depth::Record`] by default.
    pub fn depth(mut self, depth: Depth) -> Self {
        self.depth = depth;
        self
    }

    /// [`Self::depth`] at [`Depth::Gpu`] on `gpu`.
    pub fn gpu(self, gpu: Gpu) -> Self {
        self.depth(Depth::Gpu(gpu))
    }

    /// The time one frame of [`Harness::settle`] and [`Harness::play`] takes; 1/60 s by
    /// default.
    ///
    /// # Panics
    ///
    /// At [`Self::build`], when `dt` is zero or past the longest step an animation integrates,
    /// 0.1 s.
    pub const fn frame_dt(mut self, dt: Duration) -> Self {
        self.frame_dt = dt;
        self
    }

    /// The frames [`Harness::settle`] runs before the window reads as unsettled; 600 by default,
    /// ten seconds at 60 frames a second.
    ///
    /// # Panics
    ///
    /// At [`Self::build`], when `frames` is 0.
    pub const fn settle_frames(mut self, frames: u32) -> Self {
        self.settle_frames = frames;
        self
    }

    /// How long, in real time, [`Harness::settle`] waits for a [wake](crate::Waker) while the app
    /// [is busy](crate::App::is_busy) and asks no frame; 30 s by default.
    pub const fn background_wait(mut self, wait: Duration) -> Self {
        self.background_wait = wait;
        self
    }

    /// Open the window.
    ///
    /// # Panics
    ///
    /// As each setter says; at [`Depth::Gpu`], when either side of the size is zero or past the
    /// device's largest 2D texture, or the device cannot run Palantir's pipelines
    /// ([`DeviceRequirements`](crate::DeviceRequirements)).
    pub fn build(self) -> Harness {
        let fonts = self.options.fonts;
        let shaper = self
            .options
            .shaper_or_else(|| TextShaper::with_fonts(fonts));
        let (core, depth, target) = match &self.depth {
            Depth::Record => (
                HostCore::cpu(&self.options, shaper),
                FrameDepth::Record,
                None,
            ),
            Depth::Compose => (
                HostCore::cpu(&self.options, shaper),
                FrameDepth::Compose,
                None,
            ),
            Depth::Gpu(gpu) => {
                offscreen::assert_runs_palantir(gpu);
                let target = OwnedTarget::window(gpu, TARGET_LABEL, self.size, self.format);
                (
                    HostCore::new(gpu.clone(), &self.options, shaper),
                    FrameDepth::Paint,
                    Some(target),
                )
            }
        };
        self.open(core, depth, target, WINDOW)
    }

    fn open(
        self,
        core: HostCore,
        depth: FrameDepth,
        target: Option<OwnedTarget>,
        token: WindowToken,
    ) -> Harness {
        let scale = display::scale_factor(self.scale);
        Harness::assert_frame_dt(self.frame_dt);
        assert!(self.frame_dt > Duration::ZERO, "a frame takes some time");
        assert!(self.settle_frames > 0, "a settle runs at least one frame");
        let driver = core.open(token, self.strategy);
        Harness::open(
            core,
            driver,
            Seal {
                depth,
                target,
                physical: self.size,
                system_scale: scale,
                user_scale: self.user_scale,
                refresh_millihertz: self.refresh_millihertz,
                frame_dt: self.frame_dt,
                settle_frames: self.settle_frames,
                background_wait: self.background_wait,
            },
        )
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::gpu::surface::render_target::TargetFormat;
    #[cfg(test)]
    use crate::host::core::HostCore;
    #[cfg(test)]
    use crate::host::harness::Harness;
    #[cfg(test)]
    use crate::host::harness::harness_builder::Depth;
    use crate::host::harness::harness_builder::HarnessBuilder;
    #[cfg(test)]
    use crate::host::window_driver::FrameDepth;
    use crate::host::window_driver::PresentStrategy;
    use crate::text::shaper::TextShaper;
    #[cfg(test)]
    use crate::ui::resources::UiResources;
    #[cfg(test)]
    use crate::window::window_token::WindowToken;

    impl HarnessBuilder {
        /// Shape with the mono fallback: deterministic metrics that tests compute by hand, wrong
        /// for a width that follows a label.
        pub fn mono(self) -> Self {
            self.shaper(TextShaper::test_mono())
        }

        /// Present as the winit host does, through [`PresentStrategy::DirectAdaptive`], so a bench
        /// measures what the desktop pays.
        pub(crate) const fn swapchain_present(mut self) -> Self {
            self.strategy = PresentStrategy::DirectAdaptive;
            self
        }

        /// Opts into GPU timestamp and pipeline-statistics collection; the device needs the wgpu
        /// features.
        pub(crate) const fn collect_gpu_stats(mut self, collect: bool) -> Self {
            self.options.collect_gpu_stats = collect;
            self
        }
    }

    #[cfg(test)]
    impl HarnessBuilder {
        /// The texture format of the window at [`Depth::Gpu`](crate::Depth::Gpu); 8-bit sRGB
        /// RGBA by default.
        pub(crate) const fn format(mut self, format: TargetFormat) -> Self {
            self.format = format;
            self
        }

        /// A recording window over `resources`, as `token` among the windows that share them.
        pub(crate) fn build_over(self, resources: UiResources, token: WindowToken) -> Harness {
            let core = HostCore::over(resources, &self.options);
            let depth = match self.depth {
                Depth::Record => FrameDepth::Record,
                Depth::Compose => FrameDepth::Compose,
                Depth::Gpu(_) => unreachable!("a window over shared resources has no device"),
            };
            self.open(core, depth, None, token)
        }
    }
}
