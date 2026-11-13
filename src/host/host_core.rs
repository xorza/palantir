//! [`HostCore`]: the composition root of a host that paints: the [`HostCpu`] and the
//! [`HostDevice`], built together to share registries and one texture-dimension cap. It does not
//! own the [`WindowFrames`]s; each host pairs its own.

use crate::effect::effect_registry::EffectRegistry;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::effect_pipeline::wgpu_effect_store::WgpuEffectStore;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::host::host_cpu::HostCpu;
use crate::host::host_device::{BackendSpec, HostDevice};
use crate::host::host_options::HostOptions;
use crate::image_registry::ImageRegistry;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;
use crate::ui::window_frames::WindowFrames;
use crate::ui::window_frames::present::PresentStrategy;
use crate::window::window_token::WindowToken;
use std::rc::Rc;

/// A host's two halves, each its own type, so a host that always paints holds a device by
/// construction and a harness that paints nothing holds the [`HostCpu`] alone.
#[derive(Debug)]
pub(super) struct HostCore {
    pub(super) cpu: HostCpu,
    pub(super) device: HostDevice,
}

impl HostCore {
    /// The CPU half over `shaper`, then the device half on `gpu`, as
    /// [`HostDevice::attach`] builds it again.
    pub(super) fn new(gpu: Gpu, options: &HostOptions, shaper: TextShaper) -> Self {
        let image_store = Rc::new(WgpuImageStore::default());
        let effect_store = Rc::new(WgpuEffectStore::default());
        let mut cpu = HostCpu::over(
            UiResources::new(
                shaper,
                options.clipboard(),
                ImageRegistry::new(Rc::clone(&image_store)),
                EffectRegistry::new(Rc::clone(&effect_store)),
            ),
            options,
        );
        let spec = BackendSpec {
            image_store,
            effect_store,
            timing: options.timing,
        };
        let device = HostDevice::new(&spec, gpu, &mut cpu);
        Self { cpu, device }
    }

    /// The frames of the window `token`, presenting by `strategy`, over a render stream of its own.
    pub(super) fn open(&mut self, token: WindowToken, strategy: PresentStrategy) -> WindowFrames {
        let mut frames = self.cpu.open(token, strategy);
        self.device.attach_window(&mut frames);
        frames
    }
}

#[cfg(test)]
mod tests {
    use crate::damage::Damage;
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::frame::submission::Route;
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::test_gpu::isolated_headless_test_gpu;
    use crate::gpu::wgpu_backend::FrameTiming;
    use crate::host::host_core::HostCore;
    use crate::host::host_options::HostOptions;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::shadow::Shadow;
    use crate::shape::Shape;
    use crate::text::font_scope::FontScope;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::ui::window_frames::WindowFrames;
    use crate::ui::window_frames::present::TargetKey;
    use crate::ui::window_frames::present::{CpuFrame, FrameSpec, PresentStrategy};
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use crate::widgets::panel::Panel;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;
    use std::rc::Rc;

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
        let CpuFrame { built, .. } =
            core.device
                .frame(&mut core.cpu, &mut drawn, spec, &mut |_: &mut Ui| {});
        assert!(
            core.device
                .backend()
                .has_format_pipelines(spec.target.format),
            "the record built the target format's pipelines, so the submit compiles none"
        );
        core.device.submit(&mut drawn, target.as_target(), built);
        assert!(drawn.ui().gpu_pass_stats().last_main_pass_cpu().is_some());
        assert_eq!(idle.ui().gpu_pass_stats().last_main_pass_cpu(), None);
    }

    /// A frame has a target to reach when it draws something on a device that lives: a first frame
    /// paints; a still one holds the last frame on a swapchain and copies it where the target
    /// holds nothing; and past a loss, a first frame reaches nothing.
    #[test]
    fn a_frame_submits_what_draws_on_a_device_that_lives() {
        let gpu = isolated_headless_test_gpu();
        let mut core = HostCore::new(gpu.handles(), &OPTIONS, TextShaper::test_mono());
        let target = OwnedTarget::window(
            &gpu,
            "palantir.test.core.submits",
            UVec2::splat(8),
            TargetFormat::RGBA8_SRGB,
        );
        let spec = FrameSpec::at_rest(TargetKey::of(target.as_target()));
        let mut app = |_: &mut Ui| {};
        for (token, strategy, still_submits) in [
            (WindowToken(0), PresentStrategy::DirectAdaptive, false),
            (WindowToken(1), PresentStrategy::BackbufferCopy, true),
        ] {
            let mut window = core.open(token, strategy);
            let CpuFrame { built, .. } =
                core.device
                    .frame(&mut core.cpu, &mut window, spec, &mut app);
            assert!(core.device.will_submit(&built), "{strategy:?}: first");
            core.device.submit(&mut window, target.as_target(), built);
            let CpuFrame { built, .. } =
                core.device
                    .frame(&mut core.cpu, &mut window, spec, &mut app);
            assert_eq!(
                core.device.will_submit(&built),
                still_submits,
                "{strategy:?}: still"
            );
            core.device.submit(&mut window, target.as_target(), built);
            core.device.close(window);
        }

        gpu.lose();
        let mut window = core.open(WindowToken(2), PresentStrategy::DirectAdaptive);
        let CpuFrame { built, .. } = core
            .device
            .frame(&mut core.cpu, &mut window, spec, &mut app);
        assert!(built.path().is_some(), "a first frame builds whole");
        assert!(!core.device.will_submit(&built), "the device is lost");
    }

    /// A closing window gives back to the backend the cutout tables its plan holds, and only its own:
    /// two windows each plan a 400 × 300 shadow, radius 8 and blur 16, which pays for a table, so
    /// each holds a use of the one key; once the first closes, the second's use alone is left.
    #[test]
    fn a_closing_window_gives_back_its_cutout_tables() {
        let gpu = headless_test_gpu();
        let mut core = HostCore::new(gpu.handles(), &OPTIONS, TextShaper::test_mono());
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
                        Shape::rect()
                            .at(Rect::new(50.0, 50.0, 400.0, 300.0))
                            .corners(8.0),
                        Shadow {
                            color: RgbaF32::BLACK,
                            blur: 16.0,
                            ..Shadow::NONE
                        },
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
        let uses = |core: &mut HostCore| core.device.backend_mut().cutout_placing().uses();
        assert_eq!(uses(&mut core), 2, "a use of the one key a window");
        let [first, _second] = windows;
        core.device.close(first);
        assert_eq!(uses(&mut core), 1, "the second window's use alone");
    }

    /// A new device's backend keeps what the old one was built with: the timing the host chose,
    /// each way, and the one image store its registries write through.
    #[test]
    fn an_attach_keeps_the_timing_and_the_image_store() {
        for timing in [FrameTiming::Off, FrameTiming::On] {
            let first = isolated_headless_test_gpu();
            let mut options = OPTIONS;
            options.timing = timing;
            let mut core = HostCore::new(first.handles(), &options, TextShaper::test_mono());
            let store = Rc::clone(core.device.backend().image_store());
            first.lose();

            let second = isolated_headless_test_gpu();
            core.device.attach(second.handles(), &mut core.cpu, []);

            assert_eq!(core.device.backend().frame_timing(), timing);
            assert!(Rc::ptr_eq(&store, core.device.backend().image_store()));
        }
    }

    /// A recovery reaches every window of a host, as the windowed host attaches all of them: after
    /// a loss, each of two windows paints its own fill on the new device in its first frame.
    #[test]
    fn attaching_a_new_device_reaches_every_window() {
        const FILLS: [u32; 2] = [0xd23f7a, 0x2f6fd0];
        let size = UVec2::splat(8);
        let paint = |core: &mut HostCore, frames: &mut WindowFrames, gpu: &Gpu, fill: u32| {
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
                    .chrome(Chrome::new(RgbaF32::hex(fill)))
                    .show(ui);
            };
            let CpuFrame { built, .. } = core.device.frame(
                &mut core.cpu,
                frames,
                FrameSpec::at_rest(TargetKey::of(target.as_target())),
                &mut app,
            );
            core.device.submit(frames, target.as_target(), built);
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
                        .chrome(Chrome::new(RgbaF32::hex(FILLS[i])))
                        .show(ui, |ui| {
                            if let Some(block) = block {
                                Block::new()
                                    .id_salt("block")
                                    .at((2.0, 2.0))
                                    .size(4.0)
                                    .chrome(Chrome::new(RgbaF32::hex(block)))
                                    .show(ui);
                            }
                        });
                };
                let spec = FrameSpec::at_rest(TargetKey::of(target.as_target()));
                let CpuFrame { built, .. } =
                    core.device.frame(&mut core.cpu, window, spec, &mut app);
                if round == 2 {
                    let path = built.path().expect("a new block paints");
                    assert_eq!(path.route, Route::ViaBackbuffer);
                    assert!(
                        matches!(path.plan.damage, Damage::Partial(_)),
                        "window {i} repaints its block alone: {path:?}"
                    );
                }
                core.device.submit(window, target.as_target(), built);
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
