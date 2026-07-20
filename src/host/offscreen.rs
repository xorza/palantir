//! [`OffscreenHost`] — the headless peer of
//! [`WinitHost`](crate::WinitHost). Like `WinitHost` it owns the one
//! shared [`WgpuBackend`] and drives a [`WindowDriver`] (built from a
//! [`HostShared`]); unlike it there's no winit, no swapchain, and exactly
//! one window — it renders to a caller-supplied `wgpu::Texture`.
//! [`OffscreenHost::frame_offscreen`] accepts the same [`App`] lifecycle as
//! the windowed host, so update and replay semantics do not depend on the
//! output backend.
//!
//! A supported headless rendering entry point — render-to-texture for
//! screenshots, thumbnails, or server-side compositing — that also backs
//! the visual harness and GPU benches. It's a `pub` facade because
//! `WgpuBackend` is `pub(crate)` and can't be named from an external crate,
//! so callers drive the backend through this bundle. The two
//! cache-introspection methods stay `internals`-gated: they call gated
//! `WgpuBackend` helpers and exist only for the format-change test.

use glam::UVec2;

use crate::app::App;
use crate::debug_overlay::DebugOverlayConfig;
use crate::host::clock::{Clock, RealtimeClock};
use crate::host::shared::HostShared;
use crate::host::window_driver::{CpuFrame, PresentStrategy, WindowDriver};
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::backend::{BackendConfig, WgpuBackend};
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window::{WindowFrameState, WindowToken};
use crate::{Display, FrameReport};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OffscreenTarget {
    physical: UVec2,
    format: wgpu::TextureFormat,
}

/// One shared `WgpuBackend` + one `WindowDriver`, rendering to a
/// texture instead of a surface. The offscreen analogue of `WinitHost`.
#[derive(Debug)]
pub struct OffscreenHost {
    shared: HostShared,
    backend: WgpuBackend,
    driver: WindowDriver,
    target: Option<OffscreenTarget>,
}

/// Seals offscreen policy before allocating the backend and window driver.
#[derive(Debug)]
pub struct OffscreenHostBuilder {
    token: WindowToken,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shaper: TextShaper,
    collect_gpu_stats: bool,
    clock: Box<dyn Clock>,
    pixel_snap: bool,
}

impl OffscreenHostBuilder {
    /// Opt into GPU timestamp and pipeline-statistics collection. The supplied
    /// device must have the corresponding wgpu features enabled.
    pub fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.collect_gpu_stats = collect;
        self
    }

    /// Replace the realtime clock. A [`FixedClock`](crate::FixedClock) makes
    /// screenshots and thumbnails reproducible by holding animations at a
    /// caller-controlled phase.
    pub fn clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// Configure whether axis-aligned paint edges snap to physical pixels.
    pub fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    /// Allocate the backend and window driver from the sealed settings.
    pub fn build(self) -> OffscreenHost {
        let shared = HostShared::new(self.shaper);
        shared.windows.set_live(self.token, true);
        let backend = WgpuBackend::new(
            self.device,
            self.queue,
            shared.backend_shared(),
            BackendConfig {
                collect_gpu_stats: self.collect_gpu_stats,
            },
        );
        let driver = WindowDriver::builder(self.token, &shared, backend.max_texture_dim())
            .strategy(PresentStrategy::BackbufferCopy)
            .clock(self.clock)
            .pixel_snap(self.pixel_snap)
            .build();
        OffscreenHost {
            shared,
            backend,
            driver,
            target: None,
        }
    }
}

impl OffscreenHost {
    /// Start building an offscreen host whose single window is addressed by
    /// `token`. GPU timing defaults off, the clock defaults to realtime, and
    /// physical-pixel snapping defaults on.
    pub fn builder(
        token: WindowToken,
        device: wgpu::Device,
        queue: wgpu::Queue,
        shaper: TextShaper,
    ) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            token,
            device,
            queue,
            shaper,
            collect_gpu_stats: false,
            clock: Box::new(RealtimeClock::new()),
            pixel_snap: true,
        }
    }

    /// Mutable access to the window's `Ui` for building scenes.
    pub fn ui(&mut self) -> &mut Ui {
        &mut self.driver.ui
    }

    /// Set the app-global debug overlay for subsequent frames. The
    /// headless analogue of a `WinitHost` window toggling it via
    /// `Ui::debug_overlay_mut` — it writes the same shared diagnostics state
    /// the window's `Ui` reads.
    pub fn set_debug_overlay(&mut self, overlay: DebugOverlayConfig) {
        *self.shared.diagnostics.debug_overlay_mut() = overlay;
    }

    /// Run one offscreen application frame against `target`, filling the
    /// supplied texture even when the UI has not changed since the previous
    /// call. The target may be replaced between calls. The host's
    /// [`WindowToken`] is passed to [`App::update`] and [`App::record`], with
    /// the same once-only update and replayable record semantics as
    /// [`crate::WinitHost`]. Window open/close requests recorded by the app are
    /// discarded after rendering.
    pub fn frame_offscreen<T: App>(
        &mut self,
        target: &wgpu::Texture,
        scale_factor: f32,
        app: &mut T,
    ) -> FrameReport {
        render_frame(
            &mut self.driver,
            &mut self.backend,
            &mut self.target,
            target,
            scale_factor,
            app,
        )
    }

    /// Cloneable handle to the most-recent GPU instrumentation sample —
    /// same handle the `Ui` debug overlay reads from.
    pub fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.shared.diagnostics.pass_stats
    }
}

fn render_frame<T: App>(
    driver: &mut WindowDriver,
    backend: &mut WgpuBackend,
    current_target: &mut Option<OffscreenTarget>,
    target: &wgpu::Texture,
    scale_factor: f32,
    app: &mut T,
) -> FrameReport {
    let size = target.size();
    let target_state = OffscreenTarget {
        physical: UVec2::new(size.width, size.height),
        format: target.format(),
    };
    if note_target(current_target, target_state) {
        driver.invalidate_target();
    }
    let display = Display {
        pixel_snap: driver.pixel_snap,
        ..Display::from_physical(target_state.physical, scale_factor)
    };
    let CpuFrame { report, mode } = driver.cpu_frame(display, app);
    driver.render_to_texture(backend, target, mode);
    discard_window_output(driver);
    report
}

fn note_target(current: &mut Option<OffscreenTarget>, target: OffscreenTarget) -> bool {
    if *current == Some(target) {
        false
    } else {
        *current = Some(target);
        true
    }
}

fn discard_window_output(driver: &mut WindowDriver) {
    driver.ui.window_requests.commands.opens.clear();
    driver.ui.window_requests.commands.closes.clear();
    driver.ui.window_requests.close_vetoed = false;
    driver.ui.window_frame = WindowFrameState::default();
}

/// Cache-introspection peepholes for the visual format-change test. Gated
/// because they call `internals`-gated `WgpuBackend` helpers.
#[cfg(any(test, feature = "internals"))]
impl OffscreenHost {
    /// Whether the shared backend has built a pipeline set for `format`.
    /// Lets format-change tests confirm a new format materializes its own
    /// pipelines.
    pub fn has_format_pipelines(&self, format: wgpu::TextureFormat) -> bool {
        self.backend.has_format_pipelines(format)
    }

    /// Images resident in the GPU texture cache. Used by the format-change
    /// test to assert the cache survives a new format's pipeline build (no
    /// re-upload).
    pub fn gpu_image_cache_len(&self) -> usize {
        self.backend.gpu_image_cache_len()
    }
}

#[cfg(feature = "internals")]
pub(crate) mod test_support {
    use crate::app::test_support::RecordApp;
    use crate::host::clock::Clock;
    use crate::host::offscreen::{self, OffscreenHost, OffscreenTarget};
    use crate::host::shared::HostShared;
    use crate::host::window_driver::{PresentStrategy, WindowDriver};
    use crate::renderer::backend::{BackendConfig, WgpuBackend};
    use crate::text::TextShaper;
    use crate::ui::Ui;
    use crate::window::WindowToken;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct OffscreenWindowScratch {
        pub opens: usize,
        pub closes: usize,
        pub close_vetoed: bool,
        pub close_requested: bool,
    }

    pub fn offscreen_window_scratch(host: &OffscreenHost) -> OffscreenWindowScratch {
        OffscreenWindowScratch {
            opens: host.driver.ui.window_requests.commands.opens.len(),
            closes: host.driver.ui.window_requests.commands.closes.len(),
            close_vetoed: host.driver.ui.window_requests.close_vetoed,
            close_requested: host.driver.ui.window_frame.close_requested,
        }
    }

    /// Two window render streams sharing one backend and render resources. This is
    /// intentionally test-only: production multi-window ownership stays with
    /// `WinitHost`.
    #[derive(Debug)]
    pub struct TwoWindowOffscreenHost {
        backend: WgpuBackend,
        windows: [WindowDriver; 2],
        targets: [Option<OffscreenTarget>; 2],
    }

    impl TwoWindowOffscreenHost {
        pub fn new(
            device: wgpu::Device,
            queue: wgpu::Queue,
            shaper: TextShaper,
            clocks: [Box<dyn Clock>; 2],
        ) -> Self {
            let shared = HostShared::new(shaper);
            shared.windows.set_live(WindowToken(0), true);
            shared.windows.set_live(WindowToken(1), true);
            let backend = WgpuBackend::new(
                device,
                queue,
                shared.backend_shared(),
                BackendConfig::default(),
            );
            let max_texture_dim = backend.max_texture_dim();
            let [clock_a, clock_b] = clocks;
            let window_a = WindowDriver::builder(WindowToken(0), &shared, max_texture_dim)
                .strategy(PresentStrategy::BackbufferCopy)
                .clock(clock_a)
                .build();
            let window_b = WindowDriver::builder(WindowToken(1), &shared, max_texture_dim)
                .strategy(PresentStrategy::BackbufferCopy)
                .clock(clock_b)
                .build();
            Self {
                backend,
                windows: [window_a, window_b],
                targets: [None, None],
            }
        }

        pub fn frame_offscreen(
            &mut self,
            window: usize,
            target: &wgpu::Texture,
            scale_factor: f32,
            record: impl FnMut(&mut Ui),
        ) {
            let mut app = RecordApp::new(record);
            offscreen::render_frame(
                &mut self.windows[window],
                &mut self.backend,
                &mut self.targets[window],
                target,
                scale_factor,
                &mut app,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::UVec2;

    use crate::Display;
    use crate::app::App;
    use crate::host::clock::FixedClock;
    use crate::host::offscreen::{OffscreenTarget, discard_window_output, note_target};
    use crate::host::shared::HostShared;
    use crate::host::window_driver::WindowDriver;
    use crate::text::TextShaper;
    use crate::ui::Ui;
    use crate::window::{WindowConfig, WindowToken};

    #[derive(Debug, Default)]
    struct WindowCommandApp {
        records: usize,
    }

    impl App for WindowCommandApp {
        fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
            self.records += 1;
            let target = WindowToken(100 + self.records as u64);
            ui.open_window(target, WindowConfig::new("ignored"));
            ui.close_window(target);
            ui.keep_open();
            ui.request_relayout();
        }
    }

    #[test]
    fn target_change_tracks_size_and_format() {
        let a = OffscreenTarget {
            physical: UVec2::new(64, 48),
            format: wgpu::TextureFormat::Rgba8Unorm,
        };
        let resized = OffscreenTarget {
            physical: UVec2::new(65, 48),
            ..a
        };
        let reformatted = OffscreenTarget {
            format: wgpu::TextureFormat::Bgra8Unorm,
            ..resized
        };
        let mut current = None;

        assert!(note_target(&mut current, a));
        assert_eq!(current, Some(a));
        assert!(!note_target(&mut current, a));
        assert!(note_target(&mut current, resized));
        assert_eq!(current, Some(resized));
        assert!(note_target(&mut current, reformatted));
        assert_eq!(current, Some(reformatted));
    }

    #[test]
    fn completion_discards_replayed_window_state_and_reuses_capacity() {
        let shared = HostShared::new(TextShaper::default());
        let mut window = WindowDriver::builder(WindowToken(1), &shared, 8192)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let display = Display::from_physical(UVec2::new(64, 64), 1.0);
        let mut app = WindowCommandApp::default();
        window.ui.window_frame.close_requested = true;

        let _ = window.cpu_frame(display, &mut app);
        assert_eq!(
            app.records, 3,
            "cold-start warmup plus relayout must replay record three times"
        );
        assert_eq!(window.ui.window_requests.commands.opens.len(), 3);
        assert_eq!(window.ui.window_requests.commands.closes.len(), 3);
        assert!(window.ui.window_requests.close_vetoed);
        let open_capacity = window.ui.window_requests.commands.opens.capacity();
        let close_capacity = window.ui.window_requests.commands.closes.capacity();

        discard_window_output(&mut window);
        assert!(window.ui.window_requests.commands.opens.is_empty());
        assert!(window.ui.window_requests.commands.closes.is_empty());
        assert_eq!(
            window.ui.window_requests.commands.opens.capacity(),
            open_capacity
        );
        assert_eq!(
            window.ui.window_requests.commands.closes.capacity(),
            close_capacity
        );
        assert!(!window.ui.window_requests.close_vetoed);
        assert!(!window.ui.window_frame.close_requested);

        window.ui.request_repaint();
        let _ = window.cpu_frame(display, &mut app);
        assert_eq!(app.records, 5, "relayout must replay the warm frame once");
        assert_eq!(
            window.ui.window_requests.commands.opens.capacity(),
            open_capacity
        );
        assert_eq!(
            window.ui.window_requests.commands.closes.capacity(),
            close_capacity
        );

        discard_window_output(&mut window);
        assert!(window.ui.window_requests.commands.opens.is_empty());
        assert!(window.ui.window_requests.commands.closes.is_empty());
        assert_eq!(
            window.ui.window_requests.commands.opens.capacity(),
            open_capacity
        );
        assert_eq!(
            window.ui.window_requests.commands.closes.capacity(),
            close_capacity
        );
    }
}
