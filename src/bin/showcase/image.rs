//! Image drawing: fit modes against a 64×64 checkerboard, tint / alpha
//! variations on a gradient source, tiled repeat (UV wrapped with
//! `fract` in-shader), and linear vs nearest sampling on an upscaled
//! micro-sprite.

use crate::support;
use crate::support::{cell_row, demo_cell};
use aperture::{Color, Image, ImageFilter, ImageFit, ImageHandle, Shape, Ui};
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

/// 4×4 primary-color sprite — small enough that any cell-sized draw is
/// a heavy upscale, making the linear-vs-nearest difference obvious.
fn sprite() -> Image {
    let px: [[u8; 4]; 16] = [
        [230, 60, 60, 255],
        [230, 200, 60, 255],
        [60, 200, 90, 255],
        [60, 120, 230, 255],
        [230, 120, 60, 255],
        [240, 240, 240, 255],
        [30, 30, 30, 255],
        [140, 60, 200, 255],
        [60, 200, 200, 255],
        [30, 30, 30, 255],
        [240, 240, 240, 255],
        [200, 60, 140, 255],
        [120, 200, 60, 255],
        [60, 60, 120, 255],
        [200, 200, 120, 255],
        [120, 30, 30, 255],
    ];
    Image::from_rgba8(4, 4, px.into_iter().flatten().collect())
}

thread_local! {
    /// The demo images are permanent content, so register them once and
    /// hold the owning [`ImageHandle`]s here for the life of the process
    /// — the GPU textures live as long as these handles do. (A real app
    /// would store handles in its own state, dropping them to free VRAM.)
    static IMAGES: RefCell<Option<(ImageHandle, ImageHandle, ImageHandle)>> =
        const { RefCell::new(None) };
}

/// Clone out this frame's handles, registering on first call.
fn handles(ui: &Ui) -> (ImageHandle, ImageHandle, ImageHandle) {
    IMAGES.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| {
            (
                ui.register_image(checker()),
                ui.register_image(gradient()),
                ui.register_image(sprite()),
            )
        })
        .clone()
    })
}

pub fn build(ui: &mut Ui) {
    let (checker, gradient, sprite) = handles(ui);
    support::page(ui, |ui| {
        cell_row(ui, "fits", |ui| {
            demo_cell(ui, "fit — Fill", |ui| {
                image(ui, &checker, ImageFit::Fill, Color::WHITE);
            });
            demo_cell(ui, "fit — Contain", |ui| {
                image(ui, &checker, ImageFit::Contain, Color::WHITE);
            });
            demo_cell(ui, "fit — Cover", |ui| {
                image(ui, &checker, ImageFit::Cover, Color::WHITE);
            });
            demo_cell(ui, "fit — None", |ui| {
                image(ui, &checker, ImageFit::None, Color::WHITE);
            });
        });
        cell_row(ui, "tints", |ui| {
            demo_cell(ui, "no tint", |ui| {
                image(ui, &gradient, ImageFit::Fill, Color::WHITE);
            });
            demo_cell(ui, "red tint", |ui| {
                image(
                    ui,
                    &gradient,
                    ImageFit::Fill,
                    Color::rgba(1.0, 0.3, 0.3, 1.0),
                );
            });
            demo_cell(ui, "half alpha", |ui| {
                image(
                    ui,
                    &gradient,
                    ImageFit::Fill,
                    Color::rgba(1.0, 1.0, 1.0, 0.5),
                );
            });
        });
        cell_row(ui, "tiles", |ui| {
            demo_cell(ui, "tile 3×3", |ui| {
                let fit = ImageFit::Tile {
                    offset: Vec2::ZERO,
                    scale: Vec2::splat(3.0),
                };
                image(ui, &checker, fit, Color::WHITE);
            });
            demo_cell(ui, "tile 2×4 + offset", |ui| {
                let fit = ImageFit::Tile {
                    offset: Vec2::new(0.25, 0.0),
                    scale: Vec2::new(2.0, 4.0),
                };
                image(ui, &gradient, fit, Color::WHITE);
            });
        });
        cell_row(ui, "filters (4×4 sprite upscaled)", |ui| {
            demo_cell(ui, "filter — Linear", |ui| {
                filtered_image(ui, &sprite, ImageFilter::Linear);
            });
            demo_cell(ui, "filter — Nearest", |ui| {
                filtered_image(ui, &sprite, ImageFilter::Nearest);
            });
        });
    });
}

fn image(ui: &mut Ui, handle: &ImageHandle, fit: ImageFit, tint: Color) {
    ui.add_shape(
        Shape::image(handle.clone())
            .fit(fit)
            .filter(ImageFilter::Linear)
            .tint(tint),
    );
}

fn filtered_image(ui: &mut Ui, handle: &ImageHandle, filter: ImageFilter) {
    ui.add_shape(
        Shape::image(handle.clone())
            .fit(ImageFit::Fill)
            .filter(filter),
    );
}
