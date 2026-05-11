//! Headless wgpu device + one-frame render + texture readback into
//! an `image::RgbaImage`.

use std::sync::OnceLock;

use glam::UVec2;
use image::RgbaImage;
use palantir::{Color, DebugOverlayConfig, Display, Host, TextShaper, Ui};
use pollster::FutureExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const COPY_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
const BYTES_PER_PIXEL: u32 = 4;

/// One wgpu device + queue per test process. Both are `Send + Sync` and
/// internally `Arc`-backed, so cloning is cheap. `request_adapter` /
/// `request_device` dominate per-harness setup — sharing them turns
/// per-test wgpu init from "tens of ms" into "one clone".
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu() -> &'static Gpu {
    static G: OnceLock<Gpu> = OnceLock::new();
    G.get_or_init(|| {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .block_on()
            .expect("request adapter (headless)");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("palantir.visual_test.device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("request device");
        Gpu { device, queue }
    })
}

thread_local! {
    /// `TextShaper` is `Rc<RefCell<CosmicMeasure>>` — not `Send`, so
    /// we keep one per worker thread instead of globally. Fonts load
    /// once per thread; cargo test reuses workers across tests so the
    /// cost amortizes.
    static COSMIC: TextShaper = TextShaper::with_bundled_fonts();
}

pub struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub host: Host,
}

impl Harness {
    pub fn new() -> Self {
        let g = gpu();
        let shaper = COSMIC.with(|c| c.clone());
        let host = Host::with_text(g.device.clone(), g.queue.clone(), FORMAT, shaper);

        Self {
            device: g.device.clone(),
            queue: g.queue.clone(),
            host,
        }
    }

    pub fn render(
        &mut self,
        physical: UVec2,
        scale: f32,
        clear: Color,
        scene: impl FnMut(&mut Ui),
    ) -> RgbaImage {
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.visual_test.target"),
            size: wgpu::Extent3d {
                width: physical.x,
                height: physical.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        self.host.run_frame(
            Display::from_physical(physical, scale),
            std::time::Duration::ZERO,
            scene,
        );
        self.host.render(&target, clear);

        readback(&self.device, &self.queue, &target, physical)
    }

    /// Render `settle_frames` discards then capture the next frame.
    /// Used by fixtures whose state populates over multiple frames
    /// (scrollbars reading their populated `ScrollState`, damage
    /// seeding `DamageEngine.prev`).
    pub fn render_after_settle<F: FnMut(&mut Ui) + Copy>(
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
    pub fn render_with_overlay(
        &mut self,
        overlay: DebugOverlayConfig,
        physical: UVec2,
        scale: f32,
        clear: Color,
        scene: impl FnMut(&mut Ui),
    ) -> RgbaImage {
        self.host.debug_overlay = overlay;
        let img = self.render(physical, scale, clear, scene);
        self.host.debug_overlay = DebugOverlayConfig::default();
        img
    }
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
        label: Some("palantir.visual_test.readback"),
        size: (padded * size.y) as u64,
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
    queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("poll");
    rx.recv().expect("map_async result").expect("map ok");

    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity(row_bytes * size.y as usize);
    for y in 0..size.y as usize {
        let row_start = y * padded as usize;
        out.extend_from_slice(&data[row_start..row_start + row_bytes]);
    }
    drop(data);
    buffer.unmap();
    RgbaImage::from_raw(size.x, size.y, out).expect("buffer length matches dimensions")
}
