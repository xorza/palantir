use crate::internals::panic_probe;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::{Image, ImageFit};
use glam::{UVec2, Vec2};

/// A 100×50 image in a 200×200 rect, per fit:
/// - `Fill` keeps the full 200×200 rect (image stretched).
/// - `Contain` scales by min(200/100, 200/50) = 2 → 200×100, centred.
/// - `Cover` scales by max(200/100, 200/50) = 4 → 400×200, painted at
///   200×200 over the centred half of the width: `uv_size.x = 0.5`,
///   `uv_min.x = 0.25`.
/// - `None` paints the intrinsic 100×50, centred.
/// - `Tile` takes the raw UV and the full rect.
#[test]
fn image_fit_modes_resolve_to_expected_rects_and_uv() {
    let base = Rect::new(0.0, 0.0, 200.0, 200.0);
    let img = Vec2::new(100.0, 50.0);
    let tile = ImageFit::Tile {
        offset: Vec2::new(0.5, 0.25),
        scale: Vec2::new(3.0, 2.0),
    };
    let rows = [
        (ImageFit::Fill, img, base, Vec2::ZERO, Vec2::ONE),
        (
            ImageFit::Contain,
            img,
            Rect::new(0.0, 50.0, 200.0, 100.0),
            Vec2::ZERO,
            Vec2::ONE,
        ),
        (
            ImageFit::Cover,
            img,
            base,
            Vec2::new(0.25, 0.0),
            Vec2::new(0.5, 1.0),
        ),
        (
            ImageFit::None,
            img,
            Rect::new(50.0, 75.0, 100.0, 50.0),
            Vec2::ZERO,
            Vec2::ONE,
        ),
        (tile, img, base, Vec2::new(0.5, 0.25), Vec2::new(3.0, 2.0)),
        // No intrinsic size: the base rect at full UV.
        (ImageFit::Contain, Vec2::ZERO, base, Vec2::ZERO, Vec2::ONE),
    ];
    for (fit, intrinsic, rect, uv_min, uv_size) in rows {
        let got = fit.resolve(base, intrinsic);
        assert_eq!(
            (got.rect, got.uv_min, got.uv_size),
            (rect, uv_min, uv_size),
            "{fit:?} over {intrinsic}",
        );
    }
}

#[test]
fn image_stores_valid_rgba8_dimensions_and_pixels() {
    let pixels = vec![255, 0, 0, 255, 0, 255, 0, 128];
    let image = Image::from_srgba8(UVec2::new(2, 1), pixels.clone());
    assert_eq!(image.size(), UVec2::new(2, 1));
    assert_eq!(image.pixels, pixels);

    let blank = Image::blank(UVec2::new(2, 3));
    assert_eq!(blank.size(), UVec2::new(2, 3));
    assert_eq!(blank.pixels, vec![0; 24]);
}

#[test]
fn fill_with_visits_every_texel_in_row_major_order() {
    let mut image = Image::blank(UVec2::new(3, 2));
    let mut visited = 0;
    image.fill_with(|column, row| {
        let texel = SrgbaU8::rgb(column as u8, row as u8, visited);
        visited += 1;
        texel
    });
    assert_eq!(visited, 6);
    assert_eq!(
        image.texels(),
        &[
            SrgbaU8::rgb(0, 0, 0),
            SrgbaU8::rgb(1, 0, 1),
            SrgbaU8::rgb(2, 0, 2),
            SrgbaU8::rgb(0, 1, 3),
            SrgbaU8::rgb(1, 1, 4),
            SrgbaU8::rgb(2, 1, 5),
        ]
    );
}

#[test]
fn repeat_row_copies_one_row_into_the_others() {
    for rows in [1, 3] {
        for source in 0..rows {
            let mut image = Image::blank(UVec2::new(2, rows));
            let row = [SrgbaU8::rgb(1, 2, 3), SrgbaU8::rgb(4, 5, 6)];
            image.row_mut(source).copy_from_slice(&row);
            image.repeat_row(source);
            assert_eq!(image.texels(), row.repeat(rows as usize));
        }
    }
}

/// Debug-only: release still panics on the out-of-range copy, but
/// from the slice rather than the screen, so only the debug build
/// can pin the row it names.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "row 3 of 3")]
fn repeat_row_rejects_a_row_outside_the_image() {
    Image::blank(UVec2::new(2, 3)).repeat_row(3);
}

#[test]
fn image_rejects_invalid_rgba8_dimensions_and_lengths() {
    #[derive(Debug)]
    struct Case {
        width: u32,
        height: u32,
        len: usize,
    }

    let cases = [
        Case {
            width: 0,
            height: 1,
            len: 0,
        },
        Case {
            width: 1,
            height: 0,
            len: 0,
        },
        Case {
            width: u32::MAX,
            height: u32::MAX,
            len: 0,
        },
        Case {
            width: 2,
            height: 2,
            len: 15,
        },
    ];

    let expected = [
        "RGBA8 dimensions must be non-zero",
        "RGBA8 dimensions must be non-zero",
        "RGBA8 dimensions overflow addressable byte length",
        "RGBA8 byte length 15 does not match 2x2x4 = 16",
    ];
    for (case, expected) in cases.into_iter().zip(expected) {
        panic_probe::assert_panics_with(expected, || {
            Image::from_srgba8(UVec2::new(case.width, case.height), vec![0; case.len])
        });
    }
}
