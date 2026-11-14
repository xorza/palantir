use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::error::ImageDataError;
use crate::primitives::paint::image::{Image, ImagePlacement, fitted_size};
use glam::{UVec2, Vec2};

/// A 100×50 image in a 200×200 rect, per fit:
/// - `Fill`: the full rect (stretched).
/// - `Contain`: scale min(2, 4) = 2 → 200×100, centered.
/// - `Cover`: scale max(2, 4) = 4 → 400×200, painted 200×200 over the centered half of the width: `uv_size.x = 0.5`, `uv_min.x = 0.25`.
/// - `Natural`: intrinsic 100×50, centered.
/// - `Tile`: raw UV, full rect.
#[test]
fn image_fit_modes_resolve_to_expected_rects_and_uv() {
    let base = Rect::new(0.0, 0.0, 200.0, 200.0);
    let img = Size::new(100.0, 50.0);
    let tile = ImagePlacement::Tile {
        offset: Vec2::new(0.5, 0.25),
        scale: Vec2::new(3.0, 2.0),
    };
    let rows = [
        (
            ImagePlacement::Fit(Fit::Fill),
            img,
            base,
            Vec2::ZERO,
            Vec2::ONE,
        ),
        (
            ImagePlacement::Fit(Fit::Contain),
            img,
            Rect::new(0.0, 50.0, 200.0, 100.0),
            Vec2::ZERO,
            Vec2::ONE,
        ),
        (
            ImagePlacement::Fit(Fit::Cover),
            img,
            base,
            Vec2::new(0.25, 0.0),
            Vec2::new(0.5, 1.0),
        ),
        (
            ImagePlacement::Fit(Fit::Natural),
            img,
            Rect::new(50.0, 75.0, 100.0, 50.0),
            Vec2::ZERO,
            Vec2::ONE,
        ),
        (tile, img, base, Vec2::new(0.5, 0.25), Vec2::new(3.0, 2.0)),
        (
            ImagePlacement::Fit(Fit::Contain),
            Size::ZERO,
            base,
            Vec2::ZERO,
            Vec2::ONE,
        ),
    ];
    for (fit, intrinsic, rect, uv_min, uv_size) in rows {
        let got = fit.resolve(base, intrinsic);
        assert_eq!(
            (got.rect, got.uv_min, got.uv_size),
            (rect, uv_min, uv_size),
            "{fit:?} over {intrinsic:?}",
        );
    }
}

#[test]
fn image_stores_valid_rgba8_dimensions_and_pixels() {
    let pixels = vec![255, 0, 0, 255, 0, 255, 0, 128];
    let image = Image::from_srgba8(UVec2::new(2, 1), pixels.clone()).unwrap();
    assert_eq!(image.size(), UVec2::new(2, 1));
    assert_eq!(image.pixels, pixels);
    assert_eq!(
        image.as_srgba8(),
        pixels,
        "the bytes go out as they came in"
    );
    assert_eq!(image.texels()[1], SrgbaU8::new(0, 255, 0, 128));

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
        ImageDataError::LengthOverflow {
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

/// The row-by-row fit holds the whole-image two-pass one bit for bit, every channel of every
/// texel, on pseudo-random texels with varied alpha: wide (37 × 23 to 10 × 6), tall (23 × 37),
/// and at a ratio near one (12 × 9 to 11 × 8), where the ring is shallowest.
#[test]
fn a_fit_row_by_row_matches_the_whole_image_two_pass() {
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::image::{tent_taps, weighed};

    /// The fit as two passes over whole-image buffers: every row filtered across, then down.
    fn two_pass(image: &Image, max_side: u32) -> Vec<SrgbaU8> {
        let size = image.fitted(max_side).size();
        let (from_w, from_h) = (image.size().x as usize, image.size().y as usize);
        let (to_w, to_h) = (size.x as usize, size.y as usize);
        let linear: Vec<[f32; 4]> = image
            .texels()
            .iter()
            .map(|texel| {
                let RgbaF32 { r, g, b, a } = RgbaF32::from_srgba8(*texel);
                [r * a, g * a, b * a, a]
            })
            .collect();
        let mut rows = vec![[0.0; 4]; to_w * from_h];
        let mut taps = Vec::new();
        for x in 0..to_w {
            tent_taps(x, from_w, to_w, &mut taps);
            for y in 0..from_h {
                rows[y * to_w + x] = weighed(&taps, |i| linear[y * from_w + i]);
            }
        }
        let mut out = Vec::new();
        for y in 0..to_h {
            tent_taps(y, from_h, to_h, &mut taps);
            for x in 0..to_w {
                let [r, g, b, a] = weighed(&taps, |i| rows[i * to_w + x]);
                let straight = if a > 0.0 { a.recip() } else { 0.0 };
                out.push(RgbaF32::new(r * straight, g * straight, b * straight, a).to_srgba8());
            }
        }
        out
    }

    let mut seed = 0x9e37_79b9_u32;
    for (size, max_side) in [
        (UVec2::new(37, 23), 10),
        (UVec2::new(23, 37), 10),
        (UVec2::new(12, 9), 11),
    ] {
        let mut image = Image::blank(size);
        image.fill_with(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [r, g, b, a] = seed.to_le_bytes();
            SrgbaU8 { r, g, b, a }
        });
        assert_eq!(
            image.fitted(max_side).texels(),
            two_pass(&image, max_side),
            "{size}"
        );
    }
}

/// `texel` reads row-major, column then row; `magnified` repeats each texel into a block of the
/// factor's side, 2 × 1 at 3 becoming 6 × 3; a texel past the size panics.
#[test]
fn a_texel_reads_by_position_and_magnifies_into_a_block() {
    use crate::internals::panic_probe;

    let (red, blue) = (SrgbaU8::rgb(255, 0, 0), SrgbaU8::rgb(0, 0, 255));
    let mut image = Image::blank(UVec2::new(2, 1));
    image.texels_mut().copy_from_slice(&[red, blue]);
    assert_eq!(image.texel(UVec2::new(0, 0)), red);
    assert_eq!(image.texel(UVec2::new(1, 0)), blue);

    let big = image.magnified(3);
    assert_eq!(big.size(), UVec2::new(6, 3));
    for y in 0..3 {
        for x in 0..6 {
            let want = if x < 3 { red } else { blue };
            assert_eq!(big.texel(UVec2::new(x, y)), want, "({x}, {y})");
        }
    }
    assert_eq!(image.magnified(1), image);
    panic_probe::assert_panics_with("lies outside", || image.texel(UVec2::new(2, 0)));
    panic_probe::assert_panics_with("at least one", || image.magnified(0));
}

/// A PNG round-trips the bytes it was written from, and bytes that are no PNG are refused.
#[cfg(feature = "png")]
#[test]
fn an_image_round_trips_through_png() {
    let mut image = Image::blank(UVec2::new(3, 2));
    image.fill_with(|at| SrgbaU8::new(at.x as u8 * 80, at.y as u8 * 120, 7, 200));
    let mut bytes = Vec::new();
    image.write_png(&mut bytes).unwrap();
    assert_eq!(Image::from_png(&bytes).unwrap(), image);
    assert!(Image::from_png(b"not a png").is_err());
}
