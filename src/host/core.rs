//! [`HostCore`]: the composition root of a host that paints: the [`CpuCore`] and the
//! [`DeviceCore`], built together to share registries and one texture-dimension cap. It does not
//! own the [`WindowDriver`]s; each host pairs its own.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::host::cpu_core::CpuCore;
use crate::host::device_core::{BackendSpec, DeviceCore};
use crate::host::host_options::HostOptions;
use crate::host::window_driver::{PresentStrategy, WindowDriver};
use crate::image_registry::ImageRegistry;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;
use std::rc::Rc;

/// A host's two halves, each its own type, so a host that always paints holds a device by
/// construction and a harness that paints nothing holds the [`CpuCore`] alone.
#[derive(Debug)]
pub(super) struct HostCore {
    pub(super) cpu: CpuCore,
    pub(super) device: DeviceCore,
}

impl HostCore {
    /// The CPU half over `shaper`, then the device half on `gpu`, as
    /// [`DeviceCore::attach`] builds it again.
    pub(super) fn new(gpu: Gpu, options: &HostOptions, shaper: TextShaper) -> Self {
        let image_store = Rc::new(WgpuImageStore::default());
        let mut cpu = CpuCore::over(
            UiResources::new(
                shaper,
                options.clipboard(),
                ImageRegistry::new(Rc::clone(&image_store)),
            ),
            options,
        );
        let spec = BackendSpec {
            image_store,
            timing: options.timing,
        };
        let device = DeviceCore::new(spec, gpu, &mut cpu);
        Self { cpu, device }
    }

    /// A driver for the window `token`, presenting by `strategy`, over a render stream of its own.
    pub(super) fn open(&mut self, token: WindowToken, strategy: PresentStrategy) -> WindowDriver {
        let mut driver = self.cpu.open(token, strategy);
        self.device.attach_window(&mut driver);
        driver
    }
}

#[cfg(test)]
mod tests {
    use crate::damage::Damage;
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutMode;
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::test_gpu::isolated_headless_test_gpu;
    use crate::host::core::HostCore;
    use crate::host::host_options::HostOptions;
    use crate::host::window_driver::TargetKey;
    use crate::host::window_driver::WindowDriver;
    use crate::host::window_driver::{CpuFrame, FrameSpec, PresentPath, PresentStrategy};
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::shadow::Shadow;
    use crate::shape::Shape;
    use crate::text::font_scope::FontScope;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use crate::widgets::panel::Panel;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;

    /// What every test core is built with: bundled fonts, no GPU timing.
    const OPTIONS: HostOptions = HostOptions::new(FontScope::Bundled);

    /// Two windows on one host render as two owners and read their own GPU samples: a submit
    /// publishes into its own window's handle, so a window that has not drawn reads nothing. The
    /// record of a frame builds its format's pipelines, ahead of its submit.
    #[test]
    fn each_window_reads_its_own_gpu_sample() {
        let gpu = headless_test_gpu();
        let mut core = HostCore::new(gpu.handles(), &OPTIONS, TextShaper::test_mono());
        let mut drawn = core.open(WindowToken(0), PresentStrategy::BackbufferCopy);
        let idle = core.open(WindowToken(1), PresentStrategy::BackbufferCopy);
        assert_ne!(drawn.render_owner(), idle.render_owner());
        let size = UVec2::splat(16);
        let target =
            OwnedTarget::window(&gpu, "palantir.test.core", size, TargetFormat::RGBA8_SRGB);
        let spec = FrameSpec::at_rest(TargetKey::of(target.as_target()));
        let CpuFrame { mode, .. } =
            core.device
                .frame(&mut core.cpu, &mut drawn, spec, &mut |_: &mut Ui| {});
        assert!(
            core.device
                .backend()
                .has_format_pipelines(spec.target.format),
            "the record built the target format's pipelines, so the submit compiles none"
        );
        core.device
            .submit(&mut core.cpu, &mut drawn, target.as_target(), mode);
        assert!(
            drawn
                .frames
                .ui
                .gpu_pass_stats()
                .last_main_pass_cpu()
                .is_some()
        );
        assert_eq!(idle.frames.ui.gpu_pass_stats().last_main_pass_cpu(), None);
    }

    /// A closing window gives back to the backend the cutout cells its plan holds, and only its own:
    /// two windows each plan a 400 × 300 shadow, radius 8 and blur 16, which pays for a table, so
    /// each holds a use of the one key; once the first closes, the second's use alone is left.
    #[test]
    fn a_closing_window_gives_back_its_cutout_cells() {
        let gpu = headless_test_gpu();
        let mut core = HostCore::new(gpu.handles(), &OPTIONS, TextShaper::test_mono());
        if core.device.backend().cutout_mode() == CutoutMode::Shade {
            return;
        }
        let size = UVec2::splat(512);
        let spec = FrameSpec::at_rest(TargetKey {
            physical: size,
            format: TargetFormat::RGBA8_SRGB,
            gives_copy: true,
        });
        let mut app = |ui: &mut Ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::shadow(Shadow {
                            color: RgbaF32::BLACK,
                            blur: 16.0,
                            ..Shadow::NONE
                        })
                        .at(Rect::new(50.0, 50.0, 400.0, 300.0))
                        .corners(8.0),
                    );
                });
        };
        let mut windows = [
            core.open(WindowToken(0), PresentStrategy::BackbufferCopy),
            core.open(WindowToken(1), PresentStrategy::BackbufferCopy),
        ];
        for window in &mut windows {
            let _ = core.device.frame(&mut core.cpu, window, spec, &mut app);
        }
        let uses = |core: &mut HostCore| core.device.backend_mut().cutout_placements_mut().uses();
        assert_eq!(uses(&mut core), 2, "a use of the one key a window");
        let [first, _second] = windows;
        core.device.close(first);
        assert_eq!(uses(&mut core), 1, "the second window's use alone");
    }

    /// A recovery reaches every window of a host, as the windowed host attaches all of them: after
    /// a loss, each of two windows paints its own fill on the new device in its first frame.
    #[test]
    fn attaching_a_new_device_reaches_every_window() {
        const FILLS: [u32; 2] = [0xd23f7a, 0x2f6fd0];
        let size = UVec2::splat(8);
        let paint = |core: &mut HostCore, driver: &mut WindowDriver, gpu: &Gpu, fill: u32| {
            let target = OwnedTarget::window(
                gpu,
                "palantir.test.core.recover",
                size,
                TargetFormat::RGBA8_SRGB,
            );
            let mut app = |ui: &mut Ui| {
                Block::new()
                    .id_salt("fill")
                    .size(Sizing::FILL)
                    .chrome(Chrome::fill(RgbaF32::hex(fill)))
                    .show(ui);
            };
            let CpuFrame { mode, .. } = core.device.frame(
                &mut core.cpu,
                driver,
                FrameSpec::at_rest(TargetKey::of(target.as_target())),
                &mut app,
            );
            core.device
                .submit(&mut core.cpu, driver, target.as_target(), mode);
            let [r, g, b, _] = target.read_back().get_pixel(4, 4).0;
            u32::from_be_bytes([0, r, g, b])
        };

        let first = isolated_headless_test_gpu();
        let mut core = HostCore::new(first.handles(), &OPTIONS, TextShaper::test_mono());
        let mut windows = [
            core.open(WindowToken(0), PresentStrategy::BackbufferCopy),
            core.open(WindowToken(1), PresentStrategy::BackbufferCopy),
        ];
        for (window, fill) in windows.iter_mut().zip(FILLS) {
            assert_eq!(paint(&mut core, window, &first, fill), fill);
        }
        first.lose();

        let second = isolated_headless_test_gpu();
        core.device
            .attach(second.handles(), &mut core.cpu, windows.iter_mut());
        for (window, fill) in windows.iter_mut().zip(FILLS) {
            assert_eq!(paint(&mut core, window, &second, fill), fill);
        }
    }

    /// Two windows of one backend take turns, each painting a partial frame between the other's:
    /// the backend's one quad stream, with its pre-clear, and its pipelines serve each window's
    /// frame alone, so each target shows its own fill around its own block, never the other's.
    #[test]
    fn two_windows_take_turns_on_one_backend() {
        const FILLS: [u32; 2] = [0xd2_3f7a, 0x2f_6fd0];
        const BLOCKS: [[u32; 2]; 2] = [[0x10_a050, 0xe0_c020], [0x60_20e0, 0x20_e0e0]];
        let size = UVec2::splat(32);
        let gpu = isolated_headless_test_gpu();
        let mut core = HostCore::new(gpu.handles(), &OPTIONS, TextShaper::test_mono());
        let mut windows = [
            core.open(WindowToken(0), PresentStrategy::BackbufferCopy),
            core.open(WindowToken(1), PresentStrategy::BackbufferCopy),
        ];
        let targets = [0, 1].map(|_| {
            OwnedTarget::window(
                &gpu,
                "palantir.test.core.turns",
                size,
                TargetFormat::RGBA8_SRGB,
            )
        });
        let rgb = |target: &OwnedTarget, x: u32, y: u32| {
            let [r, g, b, _] = target.read_back().get_pixel(x, y).0;
            u32::from_be_bytes([0, r, g, b])
        };
        for round in 0..3 {
            for (i, (window, target)) in windows.iter_mut().zip(&targets).enumerate() {
                let block = (round > 0).then(|| BLOCKS[i][round - 1]);
                let mut app = |ui: &mut Ui| {
                    Panel::canvas()
                        .id_salt("fill")
                        .size(Sizing::FILL)
                        .chrome(Chrome::fill(RgbaF32::hex(FILLS[i])))
                        .show(ui, |ui| {
                            if let Some(block) = block {
                                Block::new()
                                    .id_salt("block")
                                    .at((2.0, 2.0))
                                    .size(4.0)
                                    .chrome(Chrome::fill(RgbaF32::hex(block)))
                                    .show(ui);
                            }
                        });
                };
                let spec = FrameSpec::at_rest(TargetKey::of(target.as_target()));
                let CpuFrame { mode, .. } =
                    core.device.frame(&mut core.cpu, window, spec, &mut app);
                if round == 2 {
                    assert!(
                        matches!(mode, PresentPath::ViaBackbuffer(plan) if matches!(plan.damage, Damage::Partial(_))),
                        "window {i} repaints its block alone: {mode:?}"
                    );
                }
                core.device
                    .submit(&mut core.cpu, window, target.as_target(), mode);
                assert_eq!(
                    rgb(target, 20, 20),
                    FILLS[i],
                    "window {i} round {round}: its fill"
                );
                if let Some(block) = block {
                    assert_eq!(
                        rgb(target, 3, 3),
                        block,
                        "window {i} round {round}: its block"
                    );
                }
            }
        }
    }
}
