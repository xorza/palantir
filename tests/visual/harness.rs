//! Headless wgpu device, one-frame render and texture readback into an `image::RgbaImage`.

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

/// The palette every fixture renders under, pinned so `Palette::DEFAULT` can move without ten goldens depending on it; hue-coded so a wrong rung shows as a hue in the diff.
pub(crate) const FIXTURE_PALETTE: Palette = Palette {
    text: RgbaF32::hex(0xf2f2f2),
    text_muted: RgbaF32::hex(0x9ad2a0),
    text_disabled: RgbaF32::hex(0xd9a05e),
    window_background: RgbaF32::hex(0x14141a),
    element: RgbaF32::hex(0x2e1f38),
    element_mid: RgbaF32::hex(0x1e4048),
    element_strong: RgbaF32::hex(0x3d4f1e),
    border_focused: RgbaF32::hex(0x2f6fd0),
    accent: RgbaF32::hex(0xd23f7a),
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const COPY_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
const BYTES_PER_PIXEL: u32 = 4;

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
        Self::new_with_pixel_snap(true)
    }

    pub(crate) fn new_with_pixel_snap(pixel_snap: bool) -> Self {
        let gpu = headless_test_gpu();
        // Fresh target texture per frame: fill it via the public backbuffer+copy path.
        // A fixed clock makes goldens reproducible (the spinner at angle 0).
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

    /// The target format, `Rgba8UnormSrgb` until set; pixels come back in RGBA order (BGRA is swizzled on readback).
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
