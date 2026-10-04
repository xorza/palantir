//! Headless wgpu device + one-frame render + texture readback into
//! an `image::RgbaImage`.

#![expect(
    clippy::disallowed_types,
    reason = "an outside consumer of the published surface, where naming a wgpu type is the point"
)]

use std::sync::mpsc;
use std::time::Duration;

use glam::UVec2;
use palantir::golden::image::RgbaImage;
use palantir::internals::record_app::RecordApp;
use palantir::internals::{HeadlessTestGpuLease, headless_test_gpu};
use palantir::{
    DebugOverlayConfig, FixedClock, FramePaint, OffscreenHost, Palette, RenderTarget, RgbaF32,
    TextShaper, Theme, Ui,
};

use crate::fixtures::DARK_BG;
use std::iter;

/// The palette every fixture renders under, pinned so that
/// `Palette::DEFAULT` is free to move. Which colours the crate ships is a
/// design choice rather than something this suite tests, and without the
/// pin ten goldens sit on that choice.
///
/// Hue-coded rather than grayscale, so a recipe that reaches for the wrong
/// rung shows in the diff image as a hue instead of as nine levels of
/// gray. Inks are light and descend in luminance, surfaces are dark and
/// ascend, and the accent pair is the only saturated mid-tone.
pub(crate) const FIXTURE_PALETTE: Palette = Palette {
    text: RgbaF32::hex(0xf2f2f2),
    text_muted: RgbaF32::hex(0x9ad2a0),
    text_disabled: RgbaF32::hex(0xd9a05e),
    window_bg: RgbaF32::hex(0x14141a),
    elem: RgbaF32::hex(0x2e1f38),
    elem_mid: RgbaF32::hex(0x1e4048),
    elem_strong: RgbaF32::hex(0x3d4f1e),
    border_focused: RgbaF32::hex(0x2f6fd0),
    accent: RgbaF32::hex(0xd23f7a),
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const COPY_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
const BYTES_PER_PIXEL: u32 = 4;

/// What one captured frame drew and how it was painted.
#[derive(Debug)]
pub(crate) struct Capture {
    pub(crate) image: RgbaImage,
    /// How the frame repainted the target. A repeat render of an
    /// unchanged scene skips and copies the backbuffer, so a test about
    /// encoder replay asserts this before trusting the pixels.
    pub(crate) paint: FramePaint,
}

/// A headless host plus the surface every frame renders at: size, scale,
/// clear colour, target format and debug overlay are held here and stay
/// until changed, so a frame call names only its scene.
#[derive(Debug)]
pub(crate) struct Harness {
    pub(crate) host: OffscreenHost,
    gpu: HeadlessTestGpuLease,
    /// See [`Self::without_copy_dst`].
    target_usages: wgpu::TextureUsages,
    /// Unset until [`Self::size`]: no surface is right for every fixture.
    physical: Option<UVec2>,
    scale: f32,
    clear: RgbaF32,
    format: wgpu::TextureFormat,
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::new_with_pixel_snap(true)
    }

    pub(crate) fn new_with_pixel_snap(pixel_snap: bool) -> Self {
        let gpu = headless_test_gpu();
        // Fresh target texture per frame → must fill the whole target each
        // frame, so use the public backbuffer+copy path.
        // A fixed clock makes goldens reproducible: any animated widget (the
        // spinner's paint-time spin, caret blink, springs) samples a fixed
        // phase every run instead of a wall-clock-jittered one — the spinner
        // renders at exactly angle 0, its documented "phase 0" state.
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::new())
            .pixel_snap(pixel_snap)
            .clock(FixedClock::new(Duration::ZERO))
            .build();
        host.ui().set_theme(Theme::from_palette(&FIXTURE_PALETTE));

        Self {
            host,
            gpu,
            target_usages: HeadlessTestGpuLease::TARGET_USAGES,
            physical: None,
            scale: 1.0,
            clear: DARK_BG,
            format: FORMAT,
        }
    }

    /// Render as a target that cannot be copied into would — a GLES swapchain
    /// image. The renderer then presents its backbuffer by drawing it rather
    /// than copying it.
    pub(crate) const fn without_copy_dst(mut self) -> Self {
        self.target_usages = NO_COPY_DST_USAGES;
        self
    }

    /// The target's size in physical pixels, for this frame and the next.
    pub(crate) const fn size(&mut self, physical: UVec2) -> &mut Self {
        self.physical = Some(physical);
        self
    }

    /// The system scale the host is told, `1.0` until set.
    pub(crate) const fn scale(&mut self, scale: f32) -> &mut Self {
        self.scale = scale;
        self
    }

    /// The window clear colour, [`DARK_BG`] until set.
    pub(crate) const fn clear(&mut self, clear: RgbaF32) -> &mut Self {
        self.clear = clear;
        self
    }

    /// The target format, `Rgba8UnormSrgb` until set. Pixels come back in
    /// RGBA byte order whatever the format: BGRA targets are swizzled on
    /// readback. A change from the last frame's format is auto-detected
    /// by the renderer, which repaints in full at the new one.
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
        let target = self.gpu.target_with(
            "palantir.visual_test.target",
            physical,
            self.format,
            self.target_usages,
        );

        self.host.ui().theme_mut().window_clear = self.clear;
        let report = self.host.frame(
            RenderTarget::new(&target),
            self.scale,
            &mut RecordApp::new(scene),
        );

        let mut image = readback(&self.gpu.device, &self.gpu.queue, &target, physical);
        // Readback copies raw bytes; a BGRA target lands as B,G,R,A.
        // Swap R/B so callers always compare in RGBA space.
        if matches!(
            self.format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
        ) {
            for px in image.pixels_mut() {
                px.0.swap(0, 2);
            }
        }
        Capture {
            image,
            paint: report.paint(),
        }
    }

    /// `n` discarded frames of `scene`, for state that populates over
    /// several (scrollbars reading their `ScrollState`, damage seeding its
    /// baseline).
    pub(crate) fn prime(&mut self, n: u32, mut scene: impl FnMut(&mut Ui)) -> &mut Self {
        for _ in 0..n {
            self.frame(&mut scene);
        }
        self
    }

    /// [`Self::prime`] then [`Self::frame`], over one scene.
    pub(crate) fn settled_frame(&mut self, n: u32, mut scene: impl FnMut(&mut Ui)) -> Capture {
        self.prime(n, &mut scene);
        self.frame(scene)
    }
}

/// What a GLES swapchain image offers: it *is* the default framebuffer, so
/// nothing can be copied onto it. `COPY_SRC` is the harness's own, for
/// readback.
const NO_COPY_DST_USAGES: wgpu::TextureUsages =
    HeadlessTestGpuLease::TARGET_USAGES.difference(wgpu::TextureUsages::COPY_DST);

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    size: UVec2,
) -> RgbaImage {
    let row_bytes = (size.x * BYTES_PER_PIXEL) as usize;
    let padded = (size.x * BYTES_PER_PIXEL).div_ceil(COPY_ALIGN) * COPY_ALIGN;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("palantir.visual_test.readback"),
        size: u64::from(padded * size.y),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("palantir.visual_test.copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size.y),
            },
        },
        wgpu::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("poll");
    rx.recv().expect("map_async result").expect("map ok");

    let data = slice.get_mapped_range().expect("map readback range");
    let mut out = Vec::with_capacity(row_bytes * size.y as usize);
    for y in 0..size.y as usize {
        let row_start = y * padded as usize;
        out.extend_from_slice(&data[row_start..row_start + row_bytes]);
    }
    drop(data);
    buffer.unmap();
    RgbaImage::from_raw(size.x, size.y, out).expect("buffer length matches dimensions")
}
