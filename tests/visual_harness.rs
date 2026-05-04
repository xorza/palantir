//! Headless visual-test harness: spins up a windowless wgpu device,
//! drives one frame through `Ui` → `WgpuBackend`, copies the result
//! out to an `image::RgbaImage`. Diff + golden comparison come later
//! (see `docs/visual-testing.md`).
//!
//! Single smoke test for now — just exercises the plumbing.

use glam::UVec2;
use image::{Rgba, RgbaImage};
use palantir::{Button, Color, Configure, CosmicMeasure, Display, Sizing, Ui, WgpuBackend, share};
use pollster::FutureExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const COPY_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
const BPP: u32 = 4;

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: WgpuBackend,
    ui: Ui,
}

impl Harness {
    fn new() -> Self {
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

        let mut backend = WgpuBackend::new(device.clone(), queue.clone(), FORMAT);
        let mut ui = Ui::new();
        let cosmic = share(CosmicMeasure::with_bundled_fonts());
        ui.set_cosmic(cosmic.clone());
        backend.set_cosmic(cosmic);

        Self {
            device,
            queue,
            backend,
            ui,
        }
    }

    fn render(
        &mut self,
        physical: UVec2,
        scale: f32,
        clear: Color,
        scene: impl FnOnce(&mut Ui),
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

        self.ui.begin_frame(Display::from_physical(physical, scale));
        scene(&mut self.ui);
        let frame_out = self.ui.end_frame();
        self.backend.submit(&target, clear, frame_out);

        readback(&self.device, &self.queue, &target, physical)
    }
}

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    size: UVec2,
) -> RgbaImage {
    let unpadded = size.x * BPP;
    let padded = unpadded.div_ceil(COPY_ALIGN) * COPY_ALIGN;
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
    let mut img = RgbaImage::new(size.x, size.y);
    for y in 0..size.y {
        let row_start = (y * padded) as usize;
        for x in 0..size.x {
            let i = row_start + (x * BPP) as usize;
            img.put_pixel(x, y, Rgba([data[i], data[i + 1], data[i + 2], data[i + 3]]));
        }
    }
    drop(data);
    buffer.unmap();
    img
}

#[test]
fn headless_renders_button_scene() {
    let mut h = Harness::new();
    let size = UVec2::new(256, 96);
    let clear = Color::rgb(0.08, 0.08, 0.10);
    let img = h.render(size, 1.0, clear, |ui| {
        Button::new()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });

    assert_eq!(img.width(), size.x);
    assert_eq!(img.height(), size.y);

    // Centre pixel should differ from the clear colour — i.e. the
    // button actually painted something.
    let centre = img.get_pixel(size.x / 2, size.y / 2);
    let clear_srgb = [
        (clear.r * 255.0).round() as u8,
        (clear.g * 255.0).round() as u8,
        (clear.b * 255.0).round() as u8,
    ];
    let differs = centre.0[..3] != clear_srgb;
    assert!(
        differs,
        "centre pixel {:?} matches clear {:?} — scene didn't paint",
        centre, clear_srgb
    );
}
