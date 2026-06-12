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

use glam::UVec2;
use palantir::{
    Background, Button, Color, Configure, Corners, Frame, Image, ImageFit, Panel, Shape, Sizing,
    Stroke,
};
use wgpu::TextureFormat;

use crate::diff::{Tolerance, diff};
use crate::fixtures::DARK_BG;
use crate::harness::Harness;

/// A scene touching multiple format-dependent pipelines: a stroked,
/// rounded frame (quad pipeline) wrapping a button with a text label
/// (quad + text atlas). Both pipelines get rebuilt on the format flip,
/// so an incorrect rebuild shows up as a pixel mismatch.
fn scene(ui: &mut palantir::Ui) {
    Panel::vstack()
        .auto_id()
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Frame::new()
                .id_salt("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Color::rgb(0.20, 0.30, 0.55).into(),
                    stroke: Stroke::solid(Color::rgb(0.65, 0.80, 1.00), 2.0),
                    corners: Corners::all(12.0),
                    ..Default::default()
                })
                .show(ui);
            Button::new()
                .id_salt("btn")
                .label("format")
                .size((Sizing::FILL, Sizing::Fixed(32.0)))
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

    // Baseline at the construction format (Rgba8UnormSrgb).
    let before = h.render_to_format(TextureFormat::Rgba8UnormSrgb, size, 1.0, DARK_BG, scene);

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
    let after = h.render_to_format(TextureFormat::Bgra8UnormSrgb, size, 1.0, DARK_BG, scene);

    // Both formats are sRGB: identical perceptual output expected.
    // A small per-channel tolerance covers BGRA-vs-RGBA rounding in the
    // encode; allow a few stray pixels along AA edges of the rounded
    // stroke where the two formats can round opposite directions.
    let tol = Tolerance {
        per_channel: 2,
        max_ratio: 0.01,
    };
    let report = diff(&after, &before, tol);
    assert!(
        report.passes(tol),
        "recreated backend rendered differently after format change: \
         {} differing pixels (ratio {:.4}), max channel delta {}",
        report.differing_pixels,
        report.differing_ratio,
        report.max_channel_delta,
    );
}

/// Repeated format changes keep working: the lazy per-format pipeline map
/// caches each format's set, so flipping away and back reuses the cached
/// sets. Render at the original format again — still correct.
#[test]
fn repeated_format_changes_keep_rendering() {
    let size = UVec2::new(160, 100);
    let mut h = Harness::new();

    let baseline = h.render_to_format(TextureFormat::Rgba8UnormSrgb, size, 1.0, DARK_BG, scene);

    // Flip to a second format (auto-detected, repaints fully), then back
    // to the original — its pipeline set is still cached from the baseline
    // render above.
    let _ = h.render_to_format(TextureFormat::Bgra8UnormSrgb, size, 1.0, DARK_BG, scene);
    let restored = h.render_to_format(TextureFormat::Rgba8UnormSrgb, size, 1.0, DARK_BG, scene);

    let tol = Tolerance {
        per_channel: 2,
        max_ratio: 0.01,
    };
    let report = diff(&restored, &baseline, tol);
    assert!(
        report.passes(tol),
        "round-tripping the surface format back to the original changed the render: \
         {} differing pixels (ratio {:.4})",
        report.differing_pixels,
        report.differing_ratio,
    );
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
    Image::from_rgba8(N, N, px)
}

thread_local! {
    /// The owning handle must outlive every frame's submit (it keeps the
    /// GPU texture alive), so register once and hold it here for the
    /// whole test run — exactly what this fixture is asserting survives a
    /// format change.
    static TEST_IMAGE: std::cell::RefCell<Option<palantir::ImageHandle>> =
        const { std::cell::RefCell::new(None) };
}

/// Scene drawing the test image stretched to fill. Registers once (held
/// in `TEST_IMAGE`); later frames clone the handle.
fn image_scene(ui: &mut palantir::Ui) {
    let handle = TEST_IMAGE.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| ui.register_image(test_image()))
            .clone()
    });
    Panel::zstack()
        .auto_id()
        .padding(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            ui.add_shape(Shape::Image {
                handle,
                local_rect: None,
                fit: ImageFit::Fill,
                tint: Color::WHITE,
            });
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

    // First render at the construction format uploads the image.
    let before = h.render_to_format(
        TextureFormat::Rgba8UnormSrgb,
        size,
        1.0,
        DARK_BG,
        image_scene,
    );
    assert_eq!(
        h.host.gpu_image_cache_len(),
        1,
        "image should be resident in the GPU cache after the first render",
    );

    // Render the same image at a new format. The format change is
    // auto-detected and builds the new format's pipeline set lazily; the
    // uploaded image texture (format-independent) must survive untouched —
    // drawn from the surviving cache (count unchanged), pixel-identical.
    let after = h.render_to_format(
        TextureFormat::Bgra8UnormSrgb,
        size,
        1.0,
        DARK_BG,
        image_scene,
    );
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

    let tol = Tolerance {
        per_channel: 2,
        max_ratio: 0.01,
    };
    let report = diff(&after, &before, tol);
    assert!(
        report.passes(tol),
        "image rendered differently after format change: {} differing pixels (ratio {:.4})",
        report.differing_pixels,
        report.differing_ratio,
    );
}
