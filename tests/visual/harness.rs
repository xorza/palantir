//! Headless wgpu device + one-frame render + texture readback into
//! an `image::RgbaImage`.

use std::sync::mpsc;
use std::time::Duration;

use aperture::{
    App, Color, DebugOverlayConfig, FixedClock, HeadlessTestGpuLease, OffscreenHost, TextShaper,
    TwoWindowOffscreenHost, Ui, WindowToken, headless_test_gpu,
};
use glam::UVec2;
use image::RgbaImage;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const COPY_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
const BYTES_PER_PIXEL: u32 = 4;
const WINDOW: WindowToken = WindowToken(0);

#[derive(Debug)]
struct RecordApp<F> {
    record: F,
}

impl<F: FnMut(&mut Ui)> App for RecordApp<F> {
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        (self.record)(ui);
    }
}

thread_local! {
    /// `TextShaper` is `Rc<RefCell<CosmicMeasure>>` — not `Send`, so
    /// we keep one per worker thread instead of globally. Fonts load
    /// once per thread; cargo test reuses workers across tests so the
    /// cost amortizes.
    static COSMIC: TextShaper = TextShaper::with_bundled_fonts();
}

pub(crate) struct Harness {
    pub host: OffscreenHost,
    gpu: HeadlessTestGpuLease,
}

#[derive(Debug)]
pub(crate) struct TwoWindowHarness {
    host: TwoWindowOffscreenHost,
    gpu: HeadlessTestGpuLease,
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::new_with_pixel_snap(true)
    }

    pub(crate) fn new_with_pixel_snap(pixel_snap: bool) -> Self {
        let gpu = headless_test_gpu();
        let shaper = COSMIC.with(|c| c.clone());
        // Fresh target texture per render() → must fill the whole target each
        // frame, so use the public backbuffer+copy path.
        // A fixed clock makes goldens reproducible: any animated widget (the
        // spinner's paint-time spin, caret blink, springs) samples a fixed
        // phase every run instead of a wall-clock-jittered one — the spinner
        // renders at exactly angle 0, its documented "phase 0" state.
        let host = OffscreenHost::builder(WINDOW, gpu.device.clone(), gpu.queue.clone(), shaper)
            .pixel_snap(pixel_snap)
            .clock(FixedClock::new(Duration::ZERO))
            .build();

        Self { host, gpu }
    }

    pub(crate) fn render(
        &mut self,
        physical: UVec2,
        scale: f32,
        clear: Color,
        scene: impl FnMut(&mut Ui),
    ) -> RgbaImage {
        self.render_to_format(FORMAT, physical, scale, clear, scene)
    }

    /// Like [`Self::render`] but renders into a target texture of the
    /// given `format`, returning pixels in RGBA byte order regardless
    /// of the target's channel order (BGRA targets are swizzled on
    /// readback). A change in `format` from the previous call is
    /// auto-detected by the renderer (forces a full repaint at the new
    /// format). Used by the format-change fixture.
    pub(crate) fn render_to_format(
        &mut self,
        format: wgpu::TextureFormat,
        physical: UVec2,
        scale: f32,
        clear: Color,
        scene: impl FnMut(&mut Ui),
    ) -> RgbaImage {
        let target = make_target(&self.gpu.device, format, physical);

        self.host.ui().theme.window_clear = clear;
        self.host
            .frame_offscreen(&target, scale, &mut RecordApp { record: scene });

        let mut img = readback(&self.gpu.device, &self.gpu.queue, &target, physical);
        // Readback copies raw bytes; a BGRA target lands as B,G,R,A.
        // Swap R/B so callers always compare in RGBA space.
        if matches!(
            format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
        ) {
            for px in img.pixels_mut() {
                px.0.swap(0, 2);
            }
        }
        img
    }

    /// Render `settle_frames` discards then capture the next frame.
    /// Used by fixtures whose state populates over multiple frames
    /// (scrollbars reading their populated `ScrollState`, damage
    /// seeding `DamageEngine.prev`).
    pub(crate) fn render_after_settle<F: FnMut(&mut Ui) + Copy>(
        &mut self,
        settle_frames: u32,
        physical: UVec2,
        scale: f32,
        clear: Color,
        scene: F,
    ) -> RgbaImage {
        for _ in 0..settle_frames {
            let _ = self.render(physical, scale, clear, scene);
        }
        self.render(physical, scale, clear, scene)
    }

    /// Render one frame with `debug_overlay` set to `overlay`, then
    /// clear it again. Used by damage fixtures that flip the overlay
    /// only for the captured frame.
    pub(crate) fn render_with_overlay(
        &mut self,
        overlay: DebugOverlayConfig,
        physical: UVec2,
        scale: f32,
        clear: Color,
        scene: impl FnMut(&mut Ui),
    ) -> RgbaImage {
        self.host.set_debug_overlay(overlay);
        let img = self.render(physical, scale, clear, scene);
        self.host.set_debug_overlay(DebugOverlayConfig::default());
        img
    }
}

impl TwoWindowHarness {
    pub(crate) fn new() -> Self {
        let gpu = headless_test_gpu();
        let shaper = COSMIC.with(|c| c.clone());
        let clocks: [Box<dyn aperture::Clock>; 2] = [
            Box::new(FixedClock::new(Duration::ZERO)),
            Box::new(FixedClock::new(Duration::ZERO)),
        ];
        let host =
            TwoWindowOffscreenHost::new(gpu.device.clone(), gpu.queue.clone(), shaper, clocks);
        Self { host, gpu }
    }

    pub(crate) fn render(
        &mut self,
        window: usize,
        physical: UVec2,
        scale: f32,
        clear: Color,
        mut scene: impl FnMut(&mut Ui),
    ) -> RgbaImage {
        let target = make_target(&self.gpu.device, FORMAT, physical);
        self.host.frame_offscreen(window, &target, scale, |ui| {
            ui.theme.window_clear = clear;
            scene(ui);
        });
        readback(&self.gpu.device, &self.gpu.queue, &target, physical)
    }
}

fn make_target(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    physical: UVec2,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aperture.visual_test.target"),
        size: wgpu::Extent3d {
            width: physical.x,
            height: physical.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    size: UVec2,
) -> RgbaImage {
    let row_bytes = (size.x * BYTES_PER_PIXEL) as usize;
    let padded = (size.x * BYTES_PER_PIXEL).div_ceil(COPY_ALIGN) * COPY_ALIGN;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aperture.visual_test.readback"),
        size: (padded * size.y) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("aperture.visual_test.copy"),
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
    queue.submit(std::iter::once(encoder.finish()));

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
