use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::error::ImageDataError;
use crate::primitives::paint::image::{Image, ImageFit, fitted_size};
use glam::{UVec2, Vec2};

/// A 100×50 image in a 200×200 rect, per fit:
/// - `Fill`: the full rect (stretched).
/// - `Contain`: scale min(2, 4) = 2 → 200×100, centered.
/// - `Cover`: scale max(2, 4) = 4 → 400×200, painted 200×200 over the centered half of the width: `uv_size.x = 0.5`, `uv_min.x = 0.25`.
/// - `None`: intrinsic 100×50, centered.
/// - `Tile`: raw UV, full rect.
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
    let image = Image::from_srgba8(UVec2::new(2, 1), pixels.clone()).unwrap();
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
    image.fill_with(|at| {
        let texel = SrgbaU8::rgb(at.x as u8, at.y as u8, visited);
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

/// Debug-only: release panics on the out-of-range copy from the slice, not the named row.
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
        ImageDataError::ZeroSize {
            size: UVec2::new(0, 1),
        },
        ImageDataError::ZeroSize {
            size: UVec2::new(1, 0),
        },
        ImageDataError::TooLarge {
            size: UVec2::new(u32::MAX, u32::MAX),
        },
        ImageDataError::LengthMismatch {
            size: UVec2::new(2, 2),
            expected: 16,
            actual: 15,
        },
    ];
    for (case, expected) in cases.into_iter().zip(expected) {
        assert_eq!(
            Image::from_srgba8(UVec2::new(case.width, case.height), vec![0; case.len]),
            Err(expected),
            "{case:?}",
        );
    }
    assert_eq!(
        expected[3].to_string(),
        "RGBA8 byte length 15 does not match 2x2x4 = 16"
    );
}

/// A 4 × 2 image of black and white columns, black first, fits a `max_side` of 2 as 2 × 1. The
/// tent, widened by 2, weighs the columns at its center 3/4 each and the next ones out 1/4, over
/// the columns the image has: the left texel holds 3/4 white of 7/4, the right one 1/4 + 3/4 of
/// 7/4. In linear light that is 3/7 and 4/7, which sRGB encodes as 175 and 199; a mean of the
/// encoded bytes would give 109 and 146. An image that fits comes back as it is.
#[test]
fn a_fitted_image_averages_in_linear_light() {
    let mut image = Image::blank(UVec2::new(4, 2));
    image.fill_with(|at| {
        let v = if at.x % 2 == 0 { 0 } else { 255 };
        SrgbaU8 {
            r: v,
            g: v,
            b: v,
            a: 255,
        }
    });
    let fitted = image.fitted(2);
    assert_eq!(fitted.size(), UVec2::new(2, 1));
    let grey = |v: u8| SrgbaU8 {
        r: v,
        g: v,
        b: v,
        a: 255,
    };
    assert_eq!(fitted.texels(), [grey(175), grey(199)]);
    assert_eq!(image.fitted(4), image);
}

/// Color averages premultiplied: a transparent texel's color weighs nothing, so a 2 × 1 image of
/// opaque red beside transparent green fits to 1 × 1 as red at half alpha, not a red-green blend.
#[test]
fn a_fitted_image_weighs_color_by_alpha() {
    let mut image = Image::blank(UVec2::new(2, 1));
    image.texels_mut().copy_from_slice(&[
        SrgbaU8 {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        SrgbaU8 {
            r: 0,
            g: 255,
            b: 0,
            a: 0,
        },
    ]);
    assert_eq!(
        image.fitted(1).texels(),
        [SrgbaU8 {
            r: 255,
            g: 0,
            b: 0,
            a: 128,
        }]
    );
}

/// A fit keeps a size that fits, and scales one that does not so that its longer side is the
/// limit: 1000 of 1600 at 800 is 500, and 3 of 1001 at 10 rounds to 0.03, which keeps 1. Odd
/// sides round to the nearest: 999 of 1998 at 1000 is 499.5, which rounds up.
#[test]
fn a_fit_rounds_each_side_and_keeps_one_texel() {
    for (size, max_side, fitted) in [
        ([5, 5], 5, [5, 5]),
        ([1600, 1000], 800, [800, 500]),
        ([1000, 1600], 800, [500, 800]),
        ([1001, 3], 10, [10, 1]),
        ([1998, 999], 1000, [1000, 500]),
    ] {
        assert_eq!(
            fitted_size(UVec2::from(size), max_side),
            UVec2::from(fitted),
            "{size:?} at {max_side}"
        );
    }
}
