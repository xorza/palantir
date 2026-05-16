use palantir::{Color, Configure, Image, ImageHandle, Panel, Shape, Sizing, Ui};

/// Synthesize a 64×64 sRGB checkerboard once, register it under a
/// stable key. The framework's content-addressed `ImageRegistry`
/// idempotently dedups on the key, so calling this every frame is
/// cheap — the first frame builds + uploads; every later frame is a
/// hash lookup.
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

fn register(ui: &Ui) -> (ImageHandle, ImageHandle) {
    let checker = ui.images.register("showcase.image.checker", checker());
    let gradient = ui.images.register("showcase.image.gradient", gradient());
    (checker, gradient)
}

pub fn build(ui: &mut Ui) {
    let (checker, gradient) = register(ui);
    Panel::hstack()
        .auto_id()
        .gap(24.0)
        .padding(24.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            cell(ui, "native", |ui| {
                ui.add_shape(Shape::Image {
                    handle: checker,
                    local_rect: None,
                    tint: Color::WHITE,
                });
            });
            cell(ui, "stretched", |ui| {
                ui.add_shape(Shape::Image {
                    handle: gradient,
                    local_rect: None,
                    tint: Color::WHITE,
                });
            });
            cell(ui, "red tint", |ui| {
                ui.add_shape(Shape::Image {
                    handle: checker,
                    local_rect: None,
                    tint: Color::rgba(1.0, 0.3, 0.3, 1.0),
                });
            });
            cell(ui, "half alpha", |ui| {
                ui.add_shape(Shape::Image {
                    handle: gradient,
                    local_rect: None,
                    tint: Color::rgba(1.0, 1.0, 1.0, 0.5),
                });
            });
        });
}

fn cell(ui: &mut Ui, id: &'static str, paint: impl Fn(&mut Ui)) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .show(ui, paint);
}
