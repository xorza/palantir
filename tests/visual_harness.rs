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

/// Per-channel + ratio thresholds for [`diff`]. A pixel "differs" when
/// any of its R/G/B/A channels deviates by more than `per_channel`;
/// the overall image fails when the fraction of differing pixels
/// exceeds `max_ratio`.
#[derive(Clone, Copy, Debug)]
struct Tolerance {
    per_channel: u8,
    max_ratio: f32,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            per_channel: 2,
            max_ratio: 0.001,
        }
    }
}

#[derive(Debug)]
struct DiffReport {
    max_channel_delta: u8,
    differing_pixels: u32,
    differing_ratio: f32,
    #[allow(dead_code)] // consumed by failure-writeout in Step 4
    diff_image: RgbaImage,
}

impl DiffReport {
    fn passes(&self, tol: Tolerance) -> bool {
        self.max_channel_delta <= tol.per_channel || self.differing_ratio <= tol.max_ratio
    }
}

/// Compare two equal-sized RGBA images. The diff image marks each
/// differing pixel red (preserving alpha 255) and dims the rest of the
/// `actual` image to 25% so failures pop visually.
fn diff(actual: &RgbaImage, expected: &RgbaImage, tol: Tolerance) -> DiffReport {
    assert_eq!(
        actual.dimensions(),
        expected.dimensions(),
        "image sizes differ: actual {:?} vs expected {:?}",
        actual.dimensions(),
        expected.dimensions(),
    );
    let (w, h) = actual.dimensions();
    let mut diff_image = RgbaImage::new(w, h);
    let mut max_delta: u8 = 0;
    let mut differing: u32 = 0;
    for (a, e, d) in actual
        .pixels()
        .zip(expected.pixels())
        .zip(diff_image.pixels_mut())
        .map(|((a, e), d)| (a, e, d))
    {
        let mut pixel_delta: u8 = 0;
        for c in 0..4 {
            let dc = a.0[c].abs_diff(e.0[c]);
            if dc > pixel_delta {
                pixel_delta = dc;
            }
        }
        if pixel_delta > max_delta {
            max_delta = pixel_delta;
        }
        if pixel_delta > tol.per_channel {
            differing += 1;
            *d = Rgba([255, 0, 0, 255]);
        } else {
            *d = Rgba([a.0[0] / 4, a.0[1] / 4, a.0[2] / 4, 255]);
        }
    }
    DiffReport {
        max_channel_delta: max_delta,
        differing_pixels: differing,
        differing_ratio: differing as f32 / (w * h) as f32,
        diff_image,
    }
}

#[test]
fn diff_identical_images_passes() {
    let mut img = RgbaImage::new(8, 8);
    for p in img.pixels_mut() {
        *p = Rgba([10, 20, 30, 255]);
    }
    let report = diff(&img, &img, Tolerance::default());
    assert_eq!(report.max_channel_delta, 0);
    assert_eq!(report.differing_pixels, 0);
    assert!(report.passes(Tolerance::default()));
}

#[test]
fn diff_within_per_channel_tolerance_passes() {
    let mut a = RgbaImage::new(4, 4);
    let mut e = RgbaImage::new(4, 4);
    for p in a.pixels_mut() {
        *p = Rgba([100, 100, 100, 255]);
    }
    for p in e.pixels_mut() {
        *p = Rgba([102, 100, 100, 255]);
    }
    let report = diff(&a, &e, Tolerance::default());
    assert_eq!(report.max_channel_delta, 2);
    assert_eq!(report.differing_pixels, 0);
    assert!(report.passes(Tolerance::default()));
}

#[test]
fn diff_one_outlier_pixel_within_ratio_passes() {
    let mut a = RgbaImage::new(40, 40);
    let mut e = RgbaImage::new(40, 40);
    for p in a.pixels_mut() {
        *p = Rgba([50, 50, 50, 255]);
    }
    for p in e.pixels_mut() {
        *p = Rgba([50, 50, 50, 255]);
    }
    a.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    let report = diff(&a, &e, Tolerance::default());
    assert!(report.max_channel_delta > 2);
    assert_eq!(report.differing_pixels, 1);
    let tol = Tolerance {
        per_channel: 2,
        max_ratio: 1.0 / (40.0 * 40.0),
    };
    assert!(report.passes(tol));
}

#[test]
fn diff_too_many_outliers_fails() {
    let a = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
    let e = RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]));
    let report = diff(&a, &e, Tolerance::default());
    assert_eq!(report.max_channel_delta, 255);
    assert_eq!(report.differing_pixels, 64);
    assert!(!report.passes(Tolerance::default()));
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
