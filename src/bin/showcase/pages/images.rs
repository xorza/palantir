//! Image drawing: fit modes against a 64×64 checkerboard, tint and
//! alpha on a gradient source, tiled repeat (UVs wrapped with `fract`
//! in-shader), linear vs nearest sampling under both magnification
//! and minification, and the minification tap modes on a starfield.

use crate::support;
use crate::support::{demo_cell, demo_cell_at, section, tiles};
use glam::Vec2;
use palantir::{Color, Image, ImageDownsample, ImageFilter, ImageFit, ImageHandle, Shape, Ui};
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
        for _ in 0..W {
            pixels.extend_from_slice(&[r, g, 255, 255]);
        }
    }
    Image::from_rgba8(W, H, pixels)
}

/// 4×4 primary-colour sprite — small enough that any tile-sized draw is
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

/// 512×512 of near-black with scattered one-pixel stars — the worst case
/// for minification, and what [`ImageDownsample`] exists for. Drawn into a
/// demo cell it shrinks about 5×, so one bilinear tap reads roughly 4 of each
/// pixel's 27 source texels: most stars miss the sample grid entirely, and the
/// ones that land on it move as the image does.
///
/// Deterministic (xorshift over a fixed seed) so the page draws the same sky
/// every run.
fn starfield() -> Image {
    const N: u32 = 512;
    const STARS: usize = 900;
    let mut pixels = vec![0u8; (N * N * 4) as usize];
    for px in pixels.as_chunks_mut::<4>().0 {
        *px = [6, 8, 14, 255];
    }
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..STARS {
        let r = next();
        let x = (r % u64::from(N)) as u32;
        let y = ((r >> 20) % u64::from(N)) as u32;
        // A spread of magnitudes, and a warm/cool split, so `Peak` picking a
        // whole tap rather than a per-channel max is visible as colour kept.
        let mag = 90 + ((r >> 40) % 166) as u8;
        let warm = (r >> 48) & 1 == 0;
        let (rr, gg, bb) = if warm {
            (mag, mag.saturating_sub(30), mag.saturating_sub(70))
        } else {
            (mag.saturating_sub(60), mag.saturating_sub(20), mag)
        };
        let i = ((y * N + x) * 4) as usize;
        pixels[i..i + 4].copy_from_slice(&[rr, gg, bb, 255]);
    }
    Image::from_rgba8(N, N, pixels)
}

/// The four demo images, registered once and held for the life of the
/// process — the GPU textures live as long as these handles do. A real
/// app would store handles in its own state, dropping them to free VRAM.
#[derive(Debug)]
struct Sources {
    checker: ImageHandle,
    gradient: ImageHandle,
    sprite: ImageHandle,
    starfield: ImageHandle,
}

thread_local! {
    static IMAGES: RefCell<Option<Sources>> = const { RefCell::new(None) };
}

/// Clone out this frame's handles, registering on first call.
fn sources(ui: &Ui) -> Sources {
    IMAGES.with_borrow_mut(|slot| {
        let s = slot.get_or_insert_with(|| Sources {
            checker: ui
                .register_image(checker())
                .expect("showcase checker fits every supported GPU"),
            gradient: ui
                .register_image(gradient())
                .expect("showcase gradient fits every supported GPU"),
            sprite: ui
                .register_image(sprite())
                .expect("showcase sprite fits every supported GPU"),
            starfield: ui
                .register_image(starfield())
                .expect("showcase starfield fits every supported GPU"),
        });
        Sources {
            checker: s.checker.clone(),
            gradient: s.gradient.clone(),
            sprite: s.sprite.clone(),
            starfield: s.starfield.clone(),
        }
    })
}

pub(crate) fn build(ui: &mut Ui) {
    let src = sources(ui);

    section(
        ui,
        "fit — how the source is mapped onto a non-square destination rect",
        |ui| {
            tiles(ui, |ui| {
                for (label, fit) in [
                    ("Fill — stretch to the rect", ImageFit::Fill),
                    ("Contain — whole image, letterboxed", ImageFit::Contain),
                    ("Cover — fill the rect, crop", ImageFit::Cover),
                    ("None — 1:1, centred", ImageFit::None),
                ] {
                    demo_cell_at(ui, label, 232.0, 132.0, |ui| {
                        image(ui, &src.checker, fit, Color::WHITE);
                    });
                }
            });
        },
    );

    section(
        ui,
        "tint — the tint colour multiplies the sampled texel",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "no tint", |ui| {
                    image(ui, &src.gradient, ImageFit::Fill, Color::WHITE);
                });
                demo_cell(ui, "red tint", |ui| {
                    image(ui, &src.gradient, ImageFit::Fill, support::E);
                });
                demo_cell(ui, "half alpha", |ui| {
                    image(
                        ui,
                        &src.gradient,
                        ImageFit::Fill,
                        Color::WHITE.with_alpha(0.5),
                    );
                });
            });
        },
    );

    section(ui, "tiling — UV wrap with a scale and an offset", |ui| {
        tiles(ui, |ui| {
            demo_cell(ui, "tile 3×3", |ui| {
                let fit = ImageFit::Tile {
                    offset: Vec2::ZERO,
                    scale: Vec2::splat(3.0),
                };
                image(ui, &src.checker, fit, Color::WHITE);
            });
            demo_cell(ui, "tile 2×4, offset 0.25", |ui| {
                let fit = ImageFit::Tile {
                    offset: Vec2::new(0.25, 0.0),
                    scale: Vec2::new(2.0, 4.0),
                };
                image(ui, &src.gradient, fit, Color::WHITE);
            });
        });
    });

    section(
        ui,
        "filtering — magnification on a 4×4 sprite, minification on a 64×64 \
         checker tiled 32×",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "magnify — Linear", |ui| {
                    magnified(ui, &src.sprite, ImageFilter::Linear);
                });
                demo_cell(ui, "magnify — Nearest", |ui| {
                    magnified(ui, &src.sprite, ImageFilter::Nearest);
                });
                demo_cell(ui, "minify — Linear", |ui| {
                    minified(ui, &src.checker, ImageFilter::Linear);
                });
                demo_cell(ui, "minify — Nearest", |ui| {
                    minified(ui, &src.checker, ImageFilter::Nearest);
                });
            });
        },
    );

    section(
        ui,
        "downsampling — a 512×512 starfield minified ~5×. One tap keeps a \
         shifting subset of the stars, Mean keeps all of them and dims each by \
         the footprint area, Peak keeps them at their own brightness",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "Single (default)", |ui| {
                    downsampled(ui, &src.starfield, ImageDownsample::Single);
                });
                demo_cell(ui, "Mean", |ui| {
                    downsampled(ui, &src.starfield, ImageDownsample::Mean);
                });
                demo_cell(ui, "Peak", |ui| {
                    downsampled(ui, &src.starfield, ImageDownsample::Peak);
                });
            });
        },
    );
}

fn image(ui: &mut Ui, handle: &ImageHandle, fit: ImageFit, tint: Color) {
    ui.add_shape(Shape::image(handle.clone()).fit(fit).tint(tint));
}

fn magnified(ui: &mut Ui, handle: &ImageHandle, filter: ImageFilter) {
    ui.add_shape(
        Shape::image(handle.clone())
            .fit(ImageFit::Fill)
            .mag_filter(filter),
    );
}

fn downsampled(ui: &mut Ui, handle: &ImageHandle, downsample: ImageDownsample) {
    ui.add_shape(
        Shape::image(handle.clone())
            .fit(ImageFit::Contain)
            .downsample(downsample),
    );
}

fn minified(ui: &mut Ui, handle: &ImageHandle, filter: ImageFilter) {
    ui.add_shape(
        Shape::image(handle.clone())
            .fit(ImageFit::Tile {
                offset: Vec2::ZERO,
                scale: Vec2::splat(32.0),
            })
            .min_filter(filter),
    );
}
