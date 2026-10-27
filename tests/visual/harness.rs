//! Headless wgpu device, one-frame render and texture readback into an `image::RgbaImage`.

#![expect(
    clippy::disallowed_types,
    reason = "an outside consumer of the published surface, where naming a wgpu type is the point"
)]

use glam::UVec2;
use image::RgbaImage;
use palantir::internals::record_app::RecordApp;
use palantir::internals::{HeadlessTestGpuLease, OwnedTarget, TargetSpec, headless_test_gpu};
use palantir::{
    DebugOverlayConfig, FramePaint, OffscreenHost, OffscreenTime, Palette, RgbaF32, TextShaper,
    Theme, Ui,
};

use crate::fixtures::DARK_BG;

/// The palette every fixture renders under, pinned so `Palette::DEFAULT` can move without ten goldens depending on it; hue-coded so a wrong rung shows as a hue in the diff.
pub(crate) const FIXTURE_PALETTE: Palette = Palette {
    text: RgbaF32::hex(0xf2f2f2),
    text_muted: RgbaF32::hex(0x9ad2a0),
    text_disabled: RgbaF32::hex(0xd9a05e),
    window_bg: RgbaF32::hex(0x14141a),
    element: RgbaF32::hex(0x2e1f38),
    element_mid: RgbaF32::hex(0x1e4048),
    element_strong: RgbaF32::hex(0x3d4f1e),
    border_focused: RgbaF32::hex(0x2f6fd0),
    accent: RgbaF32::hex(0xd23f7a),
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug)]
pub(crate) struct Capture {
    pub(crate) image: RgbaImage,
    /// How the frame repainted the target; check it before trusting pixels from a repeat render, which skips and copies the backbuffer.
    pub(crate) paint: FramePaint,
}

/// A headless host plus the surface every frame renders at; settings persist until changed.
#[derive(Debug)]
pub(crate) struct Harness {
    pub(crate) host: OffscreenHost,
    gpu: HeadlessTestGpuLease,
    target_usages: wgpu::TextureUsages,
    /// Unset until [`Self::size`]: no surface is right for every fixture.
    physical: Option<UVec2>,
    scale: f32,
    clear: RgbaF32,
    format: wgpu::TextureFormat,
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::raise(true)
    }

    /// Shapes land off the pixel grid where they were authored.
    pub(crate) fn unsnapped() -> Self {
        Self::raise(false)
    }

    fn raise(pixel_snap: bool) -> Self {
        let gpu = headless_test_gpu();
        // A fixed clock makes goldens reproducible (the spinner at angle 0).
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::new())
            .pixel_snap(pixel_snap)
            .time(OffscreenTime::Manual)
            .build();
        host.ui().set_theme(Theme::from_palette(&FIXTURE_PALETTE));

        Self {
            host,
            gpu,
            target_usages: OwnedTarget::USAGES,
            physical: None,
            scale: 1.0,
            clear: DARK_BG,
            format: FORMAT,
        }
    }

    /// Render as a GLES swapchain image would: the backbuffer is drawn, not copied.
    pub(crate) const fn without_copy_dst(mut self) -> Self {
        self.target_usages = NO_COPY_DST_USAGES;
        self
    }

    pub(crate) const fn size(&mut self, physical: UVec2) -> &mut Self {
        self.physical = Some(physical);
        self
    }

    pub(crate) const fn scale(&mut self, scale: f32) -> &mut Self {
        self.scale = scale;
        self
    }

    pub(crate) const fn clear(&mut self, clear: RgbaF32) -> &mut Self {
        self.clear = clear;
        self
    }

    /// The target format, `Rgba8UnormSrgb` until set; pixels come back in RGBA order either way.
    pub(crate) const fn format(&mut self, format: wgpu::TextureFormat) -> &mut Self {
        self.format = format;
        self
    }

    /// The debug overlay every following frame draws, until set again.
    pub(crate) fn overlay(&mut self, overlay: DebugOverlayConfig) -> &mut Self {
        self.host.ui().set_debug_overlay(overlay);
        self
    }

    /// Render one frame of `scene` into a fresh target and read it back.
    pub(crate) fn frame(&mut self, scene: impl FnMut(&mut Ui)) -> Capture {
        let physical = self
            .physical
            .expect("set the surface with Harness::size before a frame");
        let target = OwnedTarget::new(
            &self.gpu.handles(),
            TargetSpec {
                label: "palantir.visual_test.target",
                size: physical,
                format: self.format,
                usage: self.target_usages,
            },
        );

        self.host.ui().theme_mut().window_clear = self.clear;
        let report = self
            .host
            .frame(target.as_target(), self.scale, &mut RecordApp::new(scene));

        let image = target.read_back();
        Capture {
            image,
            paint: report.paint(),
        }
    }

    /// `n` discarded frames of `scene`, for state that builds over several.
    pub(crate) fn prime(&mut self, n: u32, mut scene: impl FnMut(&mut Ui)) -> &mut Self {
        for _ in 0..n {
            self.frame(&mut scene);
        }
        self
    }

    pub(crate) fn settled_frame(&mut self, n: u32, mut scene: impl FnMut(&mut Ui)) -> Capture {
        self.prime(n, &mut scene);
        self.frame(scene)
    }
}

/// A GLES swapchain image: the default framebuffer, so nothing can be copied onto it; `COPY_SRC` is the harness's, for readback.
const NO_COPY_DST_USAGES: wgpu::TextureUsages =
    OwnedTarget::USAGES.difference(wgpu::TextureUsages::COPY_DST);
