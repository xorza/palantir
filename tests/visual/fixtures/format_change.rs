//! Surface format change mid-session (a window moved to an HDR output): the
//! renderer forces a full repaint, the backend keys pipelines by swapchain
//! format (`WgpuBackend::ensure_format`), and the uploaded image texture
//! survives with no re-upload.

#![expect(
    clippy::disallowed_types,
    reason = "an outside consumer of the published surface, where naming a wgpu type is the point"
)]

use glam::UVec2;
use palantir::widget::Shape;
use palantir::{
    Background, Block, Button, Configure, Corners, Image, ImageHandle, Panel, RgbaF32, Sizing,
    Stroke, TargetFormat, Ui,
};
use wgpu::TextureFormat;

use crate::goldens::assert_same;
use crate::harness::Harness;

/// A scene touching several format-dependent pipelines: a bordered rounded frame around a labelled button.
fn scene(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Block::new()
                .id_salt("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(
                    Background::rounded(RgbaF32::srgb(0.20, 0.30, 0.55), Corners::all(12.0))
                        .with_border(Stroke::new(RgbaF32::srgb(0.65, 0.80, 1.00), 2.0)),
                )
                .show(ui);
            Button::new()
                .id_salt("btn")
                .label("format")
                .size((Sizing::FILL, Sizing::fixed(32.0)))
                .show(ui);
        });
}

/// The same scene through `Rgba8UnormSrgb`, then `Bgra8UnormSrgb` with the
/// backend's pipelines rebuilt, then `Rgba8UnormSrgb` again from the cached set:
/// after BGRA reordering every render matches the first.
#[test]
fn format_changes_render_identically() {
    let mut h = Harness::new();
    h.size(UVec2::new(200, 120));
    let before = h.format(TextureFormat::Rgba8UnormSrgb).frame(scene).image;

    // Guard against a vacuous comparison: the card's centre must differ from the clear colour.
    assert_ne!(
        before.get_pixel(2, 2).0,
        before.get_pixel(100, 60).0,
        "scene drew nothing distinct from the background — comparison would be vacuous",
    );

    // `Harness::frame` swizzles the BGRA readback to RGBA.
    let swapped = h.format(TextureFormat::Bgra8UnormSrgb).frame(scene).image;
    assert_same("format_change_scene", &swapped, &before);
    let restored = h.format(TextureFormat::Rgba8UnormSrgb).frame(scene).image;
    assert_same("format_change_round_trip", &restored, &before);
}

/// A 64×64 four-quadrant image; distinct channels expose BGRA-vs-RGBA mishandling.
fn test_image() -> Image {
    const N: u32 = 64;
    const H: u32 = N / 2;
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let rgb = match (x < H, y < H) {
                (true, true) => [230, 30, 30],     // TL red
                (false, true) => [30, 230, 30],    // TR green
                (true, false) => [30, 30, 230],    // BL blue
                (false, false) => [230, 230, 230], // BR white
            };
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    Image::from_srgba8(UVec2::new(N, N), px).unwrap()
}

fn image_scene(ui: &mut Ui, image: &ImageHandle) {
    Panel::zstack()
        .auto_id()
        .padding(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            ui.add_shape(Shape::image(image.clone()));
        });
}

/// A format change rebuilds only the pipelines and keeps the uploaded image
/// texture (`Rgba8UnormSrgb`): no drop or re-upload, identical pixels.
#[test]
fn images_survive_format_change_without_reupload() {
    let mut h = Harness::new();
    let image = h
        .host
        .ui()
        .load_image(&test_image())
        .expect("fixture image fits every supported GPU");
    let scene = |ui: &mut Ui| image_scene(ui, &image);

    let before = h
        .size(UVec2::new(128, 128))
        .format(TextureFormat::Rgba8UnormSrgb)
        .frame(scene)
        .image;
    assert_eq!(
        h.host.gpu_image_cache_len(),
        1,
        "image should be resident in the GPU cache after the first render",
    );

    // The format-independent texture survives: cache count unchanged.
    let after = h.format(TextureFormat::Bgra8UnormSrgb).frame(scene).image;
    assert_eq!(
        h.host.gpu_image_cache_len(),
        1,
        "the uploaded image texture must survive a new format's pipeline build — \
         a new format adds its own pipelines only, not sampled textures, so the \
         cache must stay populated (no drop, no re-upload)",
    );
    assert!(
        h.host
            .has_format_pipelines(TargetFormat::new(TextureFormat::Bgra8UnormSrgb)),
        "the new format must have built its own pipeline set",
    );

    assert_same("format_change_image", &after, &before);
}

/// A unorm target would draw linear light too dark without error, so it is refused.
#[test]
#[should_panic(expected = "a render target's format must be sRGB or float")]
fn a_unorm_target_is_refused() {
    let mut h = Harness::new();
    let _ = h
        .size(UVec2::new(32, 32))
        .format(TextureFormat::Rgba8Unorm)
        .frame(scene)
        .image;
}
