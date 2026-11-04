//! [`HarnessBuilder`]: the settings a [`Harness`] seals at build, and the [`Depth`] its frames
//! run to.

use crate::display::system_scale::SystemScale;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::owned_target::OwnedTarget;
use crate::gpu::surface::render_target::TargetFormat;
use crate::host::core::HostCore;
use crate::host::cpu_core::CpuCore;
use crate::host::harness::{Harness, HarnessPaint, Seal};
use crate::host::headless_window::{HeadlessSettings, HeadlessWindow};
use crate::host::window_driver::{PresentStrategy, WindowDriver};
use crate::primitives::reduction::Reduction;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::theme::color_scheme::ColorScheme;
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
    /// Paint on a device into a texture of the window's size, which a shot reads.
    Gpu {
        /// The device the window paints on.
        gpu: Gpu,
        /// The texture format of the window: [`Depth::gpu`] takes 8-bit sRGB RGBA, and a float
        /// format keeps linear values past 8 bits, for a [`Harness::capture`] that blends frames.
        format: TargetFormat,
    },
}

impl Depth {
    /// [`Self::Gpu`] on `gpu` in 8-bit sRGB RGBA, the format a shot reads.
    pub const fn gpu(gpu: Gpu) -> Self {
        Self::Gpu {
            gpu,
            format: TargetFormat::RGBA8_SRGB,
        }
    }

    /// [`Self::Gpu`] on `gpu` in 16-bit float RGBA, which keeps linear values past 8 bits, for
    /// a [`Harness::capture`] that blends frames.
    pub const fn gpu_float(gpu: Gpu) -> Self {
        Self::Gpu {
            gpu,
            format: TargetFormat::RGBA16_FLOAT,
        }
    }
}

/// The settings a [`Harness`] opens with. [`Harness::builder`] sets the defaults each setter
/// names.
#[derive(Debug)]
#[must_use]
pub struct HarnessBuilder {
    size: UVec2,
    settings: HeadlessSettings,
    depth: Depth,
    strategy: PresentStrategy,
    frame_dt: Duration,
    settle_frames: u32,
    background_wait: Duration,
}

impl HarnessBuilder {
    pub(super) const fn new(size: UVec2) -> Self {
        Self {
            size,
            settings: HeadlessSettings::new(FontScope::Bundled),
            depth: Depth::Record,
            strategy: PresentStrategy::Retained,
            frame_dt: Duration::from_nanos(16_666_667),
            settle_frames: 600,
            background_wait: Duration::from_secs(30),
        }
    }

    /// Physical pixels per logical pixel, as the platform would report it; [`SystemScale::ONE`] by
    /// default. The window stays `size` physical pixels, so at 2 a 600 × 200 window is 300 × 100
    /// logical. The app's own scale goes on top through
    /// [`Ui::set_user_scale`](crate::Ui::set_user_scale); the harness's
    /// [`set_system_scale`](Harness::set_system_scale) changes this one later.
    pub const fn system_scale(mut self, scale: SystemScale) -> Self {
        self.settings.system_scale = scale;
        self
    }

    /// The monitor's refresh rate, which repaint wakes coalesce to; none by default. The
    /// harness's [`set_refresh_millihertz`](Harness::set_refresh_millihertz) changes it later.
    pub const fn refresh_millihertz(mut self, millihertz: u32) -> Self {
        self.settings.refresh_millihertz = Some(millihertz);
        self
    }

    /// The color scheme the system reports, as the winit host reports the desktop's; none by
    /// default, so [`Preference::System`](crate::Preference::System) shows the
    /// fallback. The harness's [`set_system_color_scheme`](Harness::set_system_color_scheme) changes
    /// it later.
    pub const fn system_color_scheme(mut self, scheme: ColorScheme) -> Self {
        self.settings.preferences.color_scheme = Some(scheme);
        self
    }

    /// Whether the system asks for less motion, as the winit host reports the desktop's; `false`
    /// by default. The harness's
    /// [`set_system_motion_reduced`](Harness::set_system_motion_reduced) changes it later.
    pub const fn system_motion_reduced(mut self, reduced: bool) -> Self {
        self.settings.preferences.motion = Reduction::of(reduced);
        self
    }

    /// Whether the system asks for less transparency, as the winit host reports the desktop's;
    /// `false` by default. The harness's
    /// [`set_system_transparency_reduced`](Harness::set_system_transparency_reduced) changes it
    /// later.
    pub const fn system_transparency_reduced(mut self, reduced: bool) -> Self {
        self.settings.preferences.transparency = Reduction::of(reduced);
        self
    }

    /// Whether axis-aligned paint edges snap to physical pixels; on by default.
    pub const fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.settings.options.pixel_snap = pixel_snap;
        self
    }

    /// Which faces the window shapes against; [`FontScope::Bundled`] by default, so metrics do
    /// not depend on the machine. A shaper set by [`Self::shaper`] wins over it.
    pub const fn fonts(mut self, scope: FontScope) -> Self {
        self.settings.options.fonts = scope;
        self
    }

    /// Shape through `shaper` instead of one loaded from [`Self::fonts`].
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.settings.options.shaper = Some(shaper);
        self
    }

    /// How far each frame runs; [`Depth::Record`] by default.
    pub fn depth(mut self, depth: Depth) -> Self {
        self.depth = depth;
        self
    }

    /// The time one frame of a session's [settle](crate::AgentWindow::settle) and [play](crate::AgentWindow::play) takes; 1/60 s by
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

    /// The frames that ask for another a session's [settle](crate::AgentWindow::settle) runs before the window reads as
    /// unsettled; 600 by default, ten seconds at 60 frames a second. Frames a busy app's wakes
    /// bring do not count.
    ///
    /// # Panics
    ///
    /// At [`Self::build`], when `frames` is 0.
    pub const fn settle_frames(mut self, frames: u32) -> Self {
        self.settle_frames = frames;
        self
    }

    /// How long, in real time, a session's [settle](crate::AgentWindow::settle) waits for a [wake](crate::Waker) while the app
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
        let options = &self.settings.options;
        let shaper = options.shaper_or_else(|| TextShaper::with_fonts(options.fonts));
        let token = Harness::WINDOW;
        match &self.depth {
            Depth::Record => {
                let mut cpu = CpuCore::new(options, shaper);
                let driver = cpu.open(token, self.strategy);
                let paint = HarnessPaint::Record { size: self.size };
                self.open(cpu, driver, paint)
            }
            Depth::Compose => {
                let mut cpu = CpuCore::new(options, shaper);
                let driver = cpu.open(token, self.strategy);
                let paint = HarnessPaint::Compose { size: self.size };
                self.open(cpu, driver, paint)
            }
            Depth::Gpu { gpu, format } => {
                let target = OwnedTarget::window(gpu, TARGET_LABEL, self.size, *format);
                let mut core = HostCore::new(gpu.clone(), options, shaper);
                let driver = core.open(token, self.strategy);
                let HostCore { cpu, device } = core;
                self.open(cpu, driver, HarnessPaint::Gpu { target, device })
            }
        }
    }

    fn open(self, cpu: CpuCore, driver: WindowDriver, paint: HarnessPaint) -> Harness {
        Harness::assert_frame_dt(self.frame_dt);
        assert!(self.frame_dt > Duration::ZERO, "a frame takes some time");
        assert!(self.settle_frames > 0, "a settle runs at least one frame");
        Harness::open(
            HeadlessWindow::open(cpu, driver, &self.settings),
            Seal {
                paint,
                frame_dt: self.frame_dt,
                settle_frames: self.settle_frames,
                background_wait: self.background_wait,
            },
        )
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::wgpu_backend::FrameTiming;
    #[cfg(test)]
    use crate::host::cpu_core::CpuCore;
    #[cfg(test)]
    use crate::host::harness::harness_builder::Depth;
    use crate::host::harness::harness_builder::HarnessBuilder;
    #[cfg(test)]
    use crate::host::harness::{Harness, HarnessPaint};
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
        pub(crate) const fn collect_gpu_stats(mut self, collect_gpu_stats: bool) -> Self {
            self.settings.options.timing = FrameTiming::new(collect_gpu_stats);
            self
        }
    }

    #[cfg(test)]
    impl HarnessBuilder {
        /// A recording window over `resources`, as `token` among the windows that share them.
        pub(crate) fn build_over(self, resources: UiResources, token: WindowToken) -> Harness {
            let mut cpu = CpuCore::over(resources, &self.settings.options);
            let driver = cpu.open(token, self.strategy);
            let paint = match self.depth {
                Depth::Record => HarnessPaint::Record { size: self.size },
                Depth::Compose => HarnessPaint::Compose { size: self.size },
                Depth::Gpu { .. } => unreachable!("a window over shared resources has no device"),
            };
            self.open(cpu, driver, paint)
        }
    }
}
