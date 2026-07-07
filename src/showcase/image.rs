use aperture::{Color, Configure, Image, ImageFit, ImageHandle, Panel, Shape, Sizing, Ui};
use glam::Vec2;
use std::cell::RefCell;

/// Synthesize a 64×64 sRGB checkerboard.
fn checker() -> Image {
    const N: u32 = 64;
    const CELL: u32 = 8;
    let mut pixels = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let on = ((x / CELL) ^ (y / CELL)) & 1 == 0;
            let rgb = if on { 230 } else { 30 };
            pixels.extend_from_slice(&[rgb, rgb, rgb, 255]);
        }
    }
    Image::from_rgba8(N, N, pixels)
}

/// 64×64 vertical magenta-to-cyan gradient — exercises the tint path
/// and gives a visually distinct second image.
fn gradient() -> Image {
    const W: u32 = 64;
    const H: u32 = 64;
    let mut pixels = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        let t = y as f32 / (H - 1) as f32;
        let r = (255.0 * (1.0 - t)) as u8;
        let g = (255.0 * t) as u8;
        let b = 255;
        for _ in 0..W {
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Image::from_rgba8(W, H, pixels)
}

thread_local! {
    /// The demo images are permanent content, so register them once and
    /// hold the owning [`ImageHandle`]s here for the life of the process
    /// — the GPU textures live as long as these handles do. (A real app
    /// would store handles in its own state, dropping them to free VRAM.)
    static IMAGES: RefCell<Option<(ImageHandle, ImageHandle)>> = const { RefCell::new(None) };
}

/// Clone out this frame's handles, registering on first call.
fn handles(ui: &Ui) -> (ImageHandle, ImageHandle) {
    IMAGES.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| (ui.register_image(checker()), ui.register_image(gradient())))
            .clone()
    })
}

pub fn build(ui: &mut Ui) {
    let (checker, gradient) = handles(ui);
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .padding(24.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Row 1: fit modes against a 64×64 source.
            Panel::hstack()
                .id_salt("fits")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    fit_cell(ui, "Fill", &checker, ImageFit::Fill);
                    fit_cell(ui, "Contain", &checker, ImageFit::Contain);
                    fit_cell(ui, "Cover", &checker, ImageFit::Cover);
                    fit_cell(ui, "None", &checker, ImageFit::None);
                });
            // Row 2: tint variations on the gradient.
            Panel::hstack()
                .id_salt("tints")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "no tint", |ui| {
                        image(ui, &gradient, ImageFit::Fill, Color::WHITE);
                    });
                    cell(ui, "red tint", |ui| {
                        image(
                            ui,
                            &gradient,
                            ImageFit::Fill,
                            Color::rgba(1.0, 0.3, 0.3, 1.0),
                        );
                    });
                    cell(ui, "half alpha", |ui| {
                        image(
                            ui,
                            &gradient,
                            ImageFit::Fill,
                            Color::rgba(1.0, 1.0, 1.0, 0.5),
                        );
                    });
                });
            // Row 3: tiled repeat (UV wrapped with `fract` in-shader).
            Panel::hstack()
                .id_salt("tiles")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "tile 3×3", |ui| {
                        let fit = ImageFit::Tile {
                            offset: Vec2::ZERO,
                            scale: Vec2::splat(3.0),
                        };
                        image(ui, &checker, fit, Color::WHITE);
                    });
                    cell(ui, "tile 2×4 + offset", |ui| {
                        let fit = ImageFit::Tile {
                            offset: Vec2::new(0.25, 0.0),
                            scale: Vec2::new(2.0, 4.0),
                        };
                        image(ui, &gradient, fit, Color::WHITE);
                    });
                });
        });
}

fn fit_cell(ui: &mut Ui, label: &'static str, handle: &ImageHandle, fit: ImageFit) {
    cell(ui, label, |ui| image(ui, handle, fit, Color::WHITE));
}

fn image(ui: &mut Ui, handle: &ImageHandle, fit: ImageFit, tint: Color) {
    ui.add_shape(Shape::Image {
        handle: handle.clone(),
        local_rect: None,
        fit,
        tint,
    });
}

fn cell(ui: &mut Ui, id: &'static str, paint: impl Fn(&mut Ui)) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .show(ui, paint);
}
