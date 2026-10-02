//! Surface format change mid-session — the window moved to an HDR /
//! wide-gamut output and the compositor renegotiated the swapchain's
//! color format. The renderer auto-detects the target's format change and
//! forces a full repaint at the new format.
//!
//! The shared backend keys its render pipelines by swapchain format and
//! builds a new format's set lazily on first submit
//! (`WgpuBackend::ensure_format`); the per-window backbuffer self-heals
//! (recreates) on a format change. These fixtures pin that a new format
//! produces a working renderer rendering identical perceptual pixels,
//! and that format-independent resources (the uploaded image texture)
//! survive the switch with no re-upload.

// Reaches Palantir the way an outside consumer does, through the published
// surface, where naming a wgpu type is the point. `clippy.toml` keeps them out
// of the library's own modules.
#![allow(clippy::disallowed_types)]

use glam::UVec2;
use palantir::widget::Shape;
use palantir::{
    Background, Block, Button, Configure, Corners, Image, Panel, RgbaF32, Sizing, Stroke,
};
use std::cell::RefCell;
use wgpu::TextureFormat;

use crate::goldens::assert_same;
use crate::harness::Harness;

/// A scene touching multiple format-dependent pipelines: a bordered,
/// rounded frame (quad pipeline) wrapping a button with a text label
/// (quad + text atlas). Both pipelines get rebuilt on the format flip,
/// so an incorrect rebuild shows up as a pixel mismatch.
fn scene(ui: &mut palantir::Ui) {
    Panel::vstack()
        .auto_id()
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Block::new()
                .id_salt("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: RgbaF32::srgb(0.20, 0.30, 0.55).into(),
                    border: Stroke::new(RgbaF32::srgb(0.65, 0.80, 1.00), 2.0),
                    corners: Corners::all(12.0),
                    ..Default::default()
                })
                .show(ui);
            Button::new()
                .id_salt("btn")
                .label("format")
                .size((Sizing::FILL, Sizing::fixed(32.0)))
                .show(ui);
        });
}

/// Render at the host's original sRGB format, then simulate the host
/// observing a sudden format change to a different sRGB format, recreate
/// the backend, and render the same scene again. Both formats are sRGB,
/// so the GPU's linear→sRGB encode produces the same perceptual pixels —
/// after correcting BGRA channel order the two renders must match.
/// Equivalence is the assertion: it proves the rebuilt pipelines render
/// correctly against the new format rather than panicking or drawing
/// garbage.
#[test]
fn recreate_backend_on_format_change_renders_identically() {
    let size = UVec2::new(200, 120);
    let mut h = Harness::new();

    let before = h
        .size(size)
        .format(TextureFormat::Rgba8UnormSrgb)
        .frame(scene)
        .image;

    // Guard against a vacuous comparison: the scene must actually paint
    // content distinct from the clear color, otherwise two all-clear
    // frames would match even if the rebuild drew nothing. The card's
    // center sits well inside the blue frame fill.
    let bg = before.get_pixel(2, 2);
    let center = before.get_pixel(size.x / 2, size.y / 2);
    assert_ne!(
        bg.0, center.0,
        "scene drew nothing distinct from the background — comparison would be vacuous",
    );

    // Render the same scene against the new format. The renderer notices
    // the target's format changed and forces a full repaint at the new
    // format (building its pipeline set lazily); `render_to_format`
    // swizzles the BGRA readback back into RGBA space for comparison.
    let after = h
        .size(size)
        .format(TextureFormat::Bgra8UnormSrgb)
        .frame(scene)
        .image;

    // Both formats are 8-bit sRGB on one device: the shaders write the
    // same linear values and the hardware encodes them the same way, so
    // the bytes match once the readback is swizzled.
    assert_same("format_change_scene", &after, &before);
}

/// Repeated format changes keep working: the lazy per-format pipeline map
/// caches each format's set, so flipping away and back reuses the cached
/// sets. Render at the original format again — still correct.
#[test]
fn repeated_format_changes_keep_rendering() {
    let size = UVec2::new(160, 100);
    let mut h = Harness::new();

    let baseline = h
        .size(size)
        .format(TextureFormat::Rgba8UnormSrgb)
        .frame(scene)
        .image;

    // Flip to a second format (auto-detected, repaints fully), then back
    // to the original — its pipeline set is still cached from the baseline
    // render above.
    let _ = h
        .size(size)
        .format(TextureFormat::Bgra8UnormSrgb)
        .frame(scene)
        .image;
    let restored = h
        .size(size)
        .format(TextureFormat::Rgba8UnormSrgb)
        .frame(scene)
        .image;

    assert_same("format_change_round_trip", &restored, &baseline);
}

/// A 64×64 four-quadrant image (TL red, TR green, BL blue, BR white).
/// Channel-distinct quadrants make a BGRA-vs-RGBA mishandling obvious,
/// and the hard quadrant edges survive `ImageFit::Fill` scaling.
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
    Image::from_srgba8(UVec2::new(N, N), px)
}

thread_local! {
    /// The owning handle must outlive every frame's submit (it keeps the
    /// GPU texture alive), so register once and hold it here for the
    /// whole test run — exactly what this fixture is asserting survives a
    /// format change.
    static TEST_IMAGE: std::cell::RefCell<Option<palantir::ImageHandle>> =
        const { RefCell::new(None) };
}

fn image_scene(ui: &mut palantir::Ui) {
    let handle = TEST_IMAGE.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| {
            ui.load_image(&test_image())
                .expect("fixture image fits every supported GPU")
        })
        .clone()
    });
    Panel::zstack()
        .auto_id()
        .padding(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            ui.add_shape(Shape::image(handle));
        });
}

/// The point of the surgical rebuild: a format change must rebuild only
/// the render pipelines and **keep** the uploaded image texture — the
/// image format (`Rgba8UnormSrgb`) is independent of the swapchain
/// color format. Asserts the GPU texture cache survives the flip (no
/// drop, no re-upload) and that the image still renders identically.
#[test]
fn images_survive_format_change_without_reupload() {
    let size = UVec2::new(128, 128);
    let mut h = Harness::new();

    let before = h
        .size(size)
        .format(TextureFormat::Rgba8UnormSrgb)
        .frame(image_scene)
        .image;
    assert_eq!(
        h.host.gpu_image_cache_len(),
        1,
        "image should be resident in the GPU cache after the first render",
    );

    // Render the same image at a new format. The format change is
    // auto-detected and builds the new format's pipeline set lazily; the
    // uploaded image texture (format-independent) must survive untouched —
    // drawn from the surviving cache (count unchanged), pixel-identical.
    let after = h
        .size(size)
        .format(TextureFormat::Bgra8UnormSrgb)
        .frame(image_scene)
        .image;
    assert_eq!(
        h.host.gpu_image_cache_len(),
        1,
        "the uploaded image texture must survive a new format's pipeline build — \
         a new format adds its own pipelines only, not sampled textures, so the \
         cache must stay populated (no drop, no re-upload)",
    );
    assert!(
        h.host.has_format_pipelines(TextureFormat::Bgra8UnormSrgb),
        "the new format must have built its own pipeline set",
    );

    assert_same("format_change_image", &after, &before);
}

/// A unorm target would store the renderer's linear light as is and draw
/// every colour too dark with no error, so the first frame into one
/// refuses it.
#[test]
#[should_panic(expected = "render target format Rgba8Unorm does not encode linear light")]
fn a_unorm_target_is_refused() {
    let mut h = Harness::new();
    let _ = h
        .size(UVec2::new(32, 32))
        .format(TextureFormat::Rgba8Unorm)
        .frame(scene)
        .image;
}
