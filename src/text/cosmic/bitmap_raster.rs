//! [`BitmapRaster`]: a color glyph's embedded bitmap, an `sbix` or `CBDT` strike's PNG or BGRA,
//! decoded and scaled from its strike's size to the size it draws at.

use crate::primitives::math::num::unit_to_u8;
use crate::primitives::paint::image::png_rgba;
use crate::text::cosmic::coverage_raster::{MAX_MASK_EDGE, Placement};
use glam::UVec2;
use skrifa::bitmap::{BitmapData, BitmapGlyph, Origin};

/// The Mitchell–Netravali filter's parameters, `B = C = 1/3`, the pair its authors recommend and
/// swash scaled strikes with.
const MITCHELL_B: f32 = 1.0 / 3.0;
const MITCHELL_C: f32 = 1.0 / 3.0;

/// The strike's pixels as they decode and the filter's intermediate rows, kept between glyphs.
#[derive(Debug, Default)]
pub(super) struct BitmapRaster {
    /// The strike's bitmap as straight sRGB RGBA8, which a glyph at the strike's own size takes
    /// as it is.
    decoded: Vec<u8>,
    /// [`Self::decoded`] premultiplied in `0..=1`, for the filter, which a straight image would
    /// fringe with the color of its transparent pixels.
    source: Vec<f32>,
    /// The horizontal pass: the target's width by the source's height, premultiplied.
    rows: Vec<f32>,
    taps: Vec<Tap>,
    /// Each target pixel's run of [`Self::taps`], with a trailing end.
    tap_starts: Vec<usize>,
}

/// One source pixel's weight in one target pixel.
#[derive(Clone, Copy, Debug)]
struct Tap {
    at: usize,
    weight: f32,
}

impl BitmapRaster {
    /// Draw `bitmap` at `ppem` into `out`, which it clears first, as straight sRGB RGBA8; its
    /// placement against the pen. `units_per_em` scales an `sbix` glyph's outline bearings.
    /// `None` where the data does not decode, holds no color, or scales past the largest mask.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a decoded size and a positive scale make a non-negative product"
    )]
    pub(super) fn render(
        &mut self,
        bitmap: &BitmapGlyph<'_>,
        units_per_em: u16,
        ppem: f32,
        out: &mut Vec<u8>,
    ) -> Option<Placement> {
        out.clear();
        let size = self.decode(bitmap)?;
        if size.min_element() == 0 {
            return None;
        }
        let (width, height) = (size.x, size.y);
        let strike_ppem = bitmap.ppem_y;
        if !(strike_ppem > 0.0 && ppem > 0.0) || units_per_em == 0 {
            return None;
        }
        let scale = ppem / strike_ppem;
        let target_w = (width as f32 * scale) as u32;
        let target_h = (height as f32 * scale) as u32;
        if target_w.max(target_h) > MAX_MASK_EDGE {
            return None;
        }
        // Skrifa's bearings in its strike's pixels: an `sbix` glyph sits by its outline's left side
        // bearing and its box's bottom, in font units, then its own offset; a `CBDT` glyph by its
        // top-left bearing alone, as Skia's fontations port places both.
        let font_to_strike = strike_ppem / f32::from(units_per_em);
        let left = bitmap.bearing_x * font_to_strike + bitmap.inner_bearing_x;
        let top = match bitmap.placement_origin {
            Origin::BottomLeft => {
                bitmap.bearing_y * font_to_strike + bitmap.inner_bearing_y + height as f32
            }
            Origin::TopLeft => bitmap.bearing_y * font_to_strike + bitmap.inner_bearing_y,
        };
        let placement = Placement {
            left: (left * scale).round() as i32,
            top: (top * scale).round() as i32,
            width: target_w,
            height: target_h,
        };
        if target_w == width && target_h == height {
            out.extend_from_slice(&self.decoded);
        } else {
            self.resample([width, height], [target_w, target_h], out);
        }
        Some(placement)
    }

    /// Decode `bitmap`'s data into [`Self::decoded`]; its size, `None` for data that does not
    /// decode, a BGRA block of another size than the glyph's, or a mask.
    fn decode(&mut self, bitmap: &BitmapGlyph<'_>) -> Option<UVec2> {
        self.decoded.clear();
        match bitmap.data {
            BitmapData::Png(bytes) => png_rgba::decode(bytes, &mut self.decoded).ok(),
            // Premultiplied BGRA, reordered to RGBA and divided by its alpha.
            BitmapData::Bgra(bytes) => {
                let size = UVec2::new(bitmap.width, bitmap.height);
                if bytes.len() != size.x as usize * size.y as usize * 4 {
                    return None;
                }
                self.decoded
                    .extend(bytes.as_chunks::<4>().0.iter().flat_map(|&[b, g, r, a]| {
                        unpremultiply([r, g, b, a].map(|c| f32::from(c) / 255.0))
                    }));
                Some(size)
            }
            BitmapData::Mask(_) => None,
        }
    }

    /// Scale [`Self::decoded`] from `from` to `to` with the Mitchell filter, widened by the
    /// shrink so a downscale averages every source pixel it covers, into `out`.
    fn resample(&mut self, from: [u32; 2], to: [u32; 2], out: &mut Vec<u8>) {
        let [src_w, src_h] = from.map(|n| n as usize);
        let [dst_w, dst_h] = to.map(|n| n as usize);
        self.source.clear();
        self.source.extend(
            self.decoded
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, a]| {
                    let alpha = f32::from(a) / 255.0;
                    let [r, g, b] = [r, g, b].map(|c| f32::from(c) / 255.0 * alpha);
                    [r, g, b, alpha]
                }),
        );
        // Horizontal: each source row to the target's width.
        taps(src_w, dst_w, &mut self.taps, &mut self.tap_starts);
        self.rows.clear();
        self.rows.resize(dst_w * src_h * 4, 0.0);
        for row in 0..src_h {
            let src = &self.source[row * src_w * 4..(row + 1) * src_w * 4];
            let dst = &mut self.rows[row * dst_w * 4..(row + 1) * dst_w * 4];
            for (x, px) in dst.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                for tap in &self.taps[self.tap_starts[x]..self.tap_starts[x + 1]] {
                    for (c, v) in px.iter_mut().enumerate() {
                        *v += src[tap.at * 4 + c] * tap.weight;
                    }
                }
            }
        }
        // Vertical: each target column from the horizontal pass's rows.
        taps(src_h, dst_h, &mut self.taps, &mut self.tap_starts);
        out.reserve_exact(dst_w * dst_h * 4);
        for y in 0..dst_h {
            for x in 0..dst_w {
                let mut premultiplied = [0.0f32; 4];
                for tap in &self.taps[self.tap_starts[y]..self.tap_starts[y + 1]] {
                    let px = &self.rows[(tap.at * dst_w + x) * 4..][..4];
                    for (v, &c) in premultiplied.iter_mut().zip(px) {
                        *v += c * tap.weight;
                    }
                }
                out.extend(unpremultiply(premultiplied));
            }
        }
    }
}

/// Each of `to` target pixels' taps over `from` source pixels, into `taps`, its run starting at
/// `starts[i]` and ending at `starts[i + 1]`: the Mitchell kernel at each source pixel's center,
/// stretched by the shrink, normalized to sum to one.
#[expect(
    clippy::cast_sign_loss,
    reason = "each source index is held at zero or more before the cast"
)]
fn taps(from: usize, to: usize, taps: &mut Vec<Tap>, starts: &mut Vec<usize>) {
    taps.clear();
    starts.clear();
    let scale = to as f32 / from as f32;
    let stretch = scale.recip().max(1.0);
    let support = 2.0 * stretch;
    for target in 0..to {
        starts.push(taps.len());
        let center = (target as f32 + 0.5) / scale - 0.5;
        let first = (center - support).ceil().max(0.0) as usize;
        let last = ((center + support).floor().max(0.0) as usize).min(from - 1);
        let run = taps.len();
        taps.extend((first..=last).map(|at| Tap {
            at,
            weight: mitchell((at as f32 - center) / stretch),
        }));
        // Every target center lies within half a pixel of a source center, where the kernel holds
        // at least 0.78 of its peak, so the sum is never near zero.
        let sum: f32 = taps[run..].iter().map(|tap| tap.weight).sum();
        debug_assert!(sum > 0.5, "taps of {sum} at {center}");
        for tap in &mut taps[run..] {
            tap.weight /= sum;
        }
    }
    starts.push(taps.len());
}

/// The Mitchell–Netravali cubic at `x`, zero past 2.
fn mitchell(x: f32) -> f32 {
    let (b, c) = (MITCHELL_B, MITCHELL_C);
    let x = x.abs();
    let x2 = x * x;
    let x3 = x2 * x;
    let weight = if x < 1.0 {
        (12.0 - 9.0 * b - 6.0 * c) * x3 + (-18.0 + 12.0 * b + 6.0 * c) * x2 + (6.0 - 2.0 * b)
    } else if x < 2.0 {
        (-b - 6.0 * c) * x3
            + (6.0 * b + 30.0 * c) * x2
            + (-12.0 * b - 48.0 * c) * x
            + (8.0 * b + 24.0 * c)
    } else {
        0.0
    };
    weight / 6.0
}

/// Premultiplied RGBA in `0..=1`, its overshoot clamped, as straight RGBA8.
fn unpremultiply(premultiplied: [f32; 4]) -> [u8; 4] {
    let alpha = premultiplied[3].clamp(0.0, 1.0);
    let channel = |v: f32| {
        unit_to_u8(if alpha > 0.0 {
            (v / alpha).clamp(0.0, 1.0)
        } else {
            0.0
        })
    };
    [
        channel(premultiplied[0]),
        channel(premultiplied[1]),
        channel(premultiplied[2]),
        unit_to_u8(alpha),
    ]
}

#[cfg(test)]
mod tests {
    use crate::text::cosmic::bitmap_raster::{BitmapRaster, mitchell};
    use crate::text::cosmic::coverage_raster::Placement;
    use skrifa::bitmap::{BitmapData, BitmapGlyph, MaskData, Origin};

    /// A strike glyph of `width × height` at `ppem` over `data`, its bearings zero.
    fn glyph(
        data: BitmapData<'_>,
        width: u32,
        height: u32,
        ppem: f32,
        origin: Origin,
    ) -> BitmapGlyph<'_> {
        BitmapGlyph {
            data,
            bearing_x: 0.0,
            bearing_y: 0.0,
            inner_bearing_x: 0.0,
            inner_bearing_y: 0.0,
            ppem_x: ppem,
            ppem_y: ppem,
            advance: None,
            width,
            height,
            placement_origin: origin,
        }
    }

    /// The Mitchell kernel is 8/9 at its center, 1/18 at one, zero at two and past, and its taps
    /// at the integers sum to one: 8/9 + 2 · 1/18.
    #[test]
    fn the_kernel_weighs_its_neighbors_and_sums_to_one() {
        assert!((mitchell(0.0) - 8.0 / 9.0).abs() < 1e-6);
        assert!((mitchell(1.0) - 1.0 / 18.0).abs() < 1e-6);
        assert!((mitchell(-1.0) - 1.0 / 18.0).abs() < 1e-6);
        assert_eq!(mitchell(2.0), 0.0);
        assert_eq!(mitchell(3.5), 0.0);
    }

    /// A BGRA strike at the size it draws at comes back as straight RGBA, each premultiplied
    /// channel divided by its alpha: (B 40, G 20, R 10, A 128) is (20, 40, 80, 128) straight,
    /// rounded; a PNG strike byte for byte, a faint pixel's color included. A block of another
    /// size than the glyph's, a mask and an empty glyph draw nothing.
    #[test]
    fn a_strike_at_its_own_size_comes_back_straight() {
        let bgra = [40, 20, 10, 128, 0, 0, 255, 255];
        let mut out = Vec::new();
        let mut raster = BitmapRaster::default();
        let placement = raster
            .render(
                &glyph(BitmapData::Bgra(&bgra), 2, 1, 20.0, Origin::TopLeft),
                1000,
                20.0,
                &mut out,
            )
            .expect("a BGRA strike");
        assert_eq!(placement.width, 2);
        assert_eq!(placement.height, 1);
        assert_eq!(out, [20, 40, 80, 128, 255, 0, 0, 255]);

        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[1, 2, 3, 255, 10, 20, 30, 3])
            .unwrap();
        writer.finish().unwrap();
        raster
            .render(
                &glyph(BitmapData::Png(&png), 2, 1, 20.0, Origin::TopLeft),
                1000,
                20.0,
                &mut out,
            )
            .expect("a PNG strike");
        assert_eq!(out, [1, 2, 3, 255, 10, 20, 30, 3]);

        let short = glyph(BitmapData::Bgra(&bgra[..4]), 2, 1, 20.0, Origin::TopLeft);
        assert_eq!(raster.render(&short, 1000, 20.0, &mut out), None);
        let mask = glyph(
            BitmapData::Mask(MaskData {
                bpp: 8,
                is_packed: false,
                data: &[255],
            }),
            1,
            1,
            20.0,
            Origin::TopLeft,
        );
        assert_eq!(raster.render(&mask, 1000, 20.0, &mut out), None);
        let empty = glyph(BitmapData::Bgra(&[]), 0, 0, 20.0, Origin::TopLeft);
        assert_eq!(raster.render(&empty, 1000, 20.0, &mut out), None);
    }

    /// Halving a strike halves its size and filters each target pixel over the four source
    /// columns the stretched kernel reaches, renormalized where the border cuts it. Opaque columns
    /// black, white, black, white: the first target pixel, centered at source 0.5, weighs them by
    /// `m(±0.25) = 0.7821`, `m(0.75) = 0.2561` and `m(1.25) = −0.0234` (each over the stretch of
    /// 2), so white holds `(0.7821 − 0.0234) / 1.7969 = 0.4222` of it, 108; the second, centered
    /// at 2.5, holds `(0.2561 + 0.7821) / 1.7969 = 0.5778`, 147. Uniform color stays exact at any
    /// scale, alpha included; doubling doubles the size.
    #[test]
    fn scaling_averages_and_keeps_uniform_color_exact() {
        let mut bgra = Vec::new();
        for _row in 0..4 {
            for col in 0..4 {
                let v = if col % 2 == 0 { 0 } else { 255 };
                bgra.extend([v, v, v, 255]);
            }
        }
        let mut out = Vec::new();
        let mut raster = BitmapRaster::default();
        let placement = raster
            .render(
                &glyph(BitmapData::Bgra(&bgra), 4, 4, 20.0, Origin::TopLeft),
                1000,
                10.0,
                &mut out,
            )
            .expect("a strike");
        assert_eq!([placement.width, placement.height], [2, 2]);
        assert_eq!(out, [108, 108, 108, 255, 147, 147, 147, 255].repeat(2));

        // Premultiplied R 60, G 30, B 15 at alpha 120: straight 127.5, 63.75 and 31.875 of 255.
        let uniform: Vec<u8> = [15, 30, 60, 120].repeat(9);
        for (ppem, edge) in [(10.0, 1), (40.0, 6)] {
            let placement = raster
                .render(
                    &glyph(BitmapData::Bgra(&uniform), 3, 3, 20.0, Origin::TopLeft),
                    1000,
                    ppem,
                    &mut out,
                )
                .expect("a strike");
            assert_eq!(
                [placement.width, placement.height],
                [edge, edge],
                "ppem {ppem}"
            );
            for px in out.as_chunks::<4>().0 {
                assert_eq!(*px, [128, 64, 32, 120], "ppem {ppem}");
            }
        }
    }

    /// A `CBDT` glyph sits by its top-left bearing; an `sbix` glyph by its outline's left side
    /// bearing and bottom, in font units, then its own offset, its top a height above: at a
    /// 20-ppem strike of a 1000-unit em, a bearing of (100, -200) units is (2, -4) px, the offset
    /// (1, 3) moves it to (3, -1), and a 4-row bitmap's top is 3. Drawn at 40 ppem every bearing
    /// doubles.
    #[test]
    fn each_origin_places_its_bitmap_by_its_bearings() {
        let bgra = [0, 0, 0, 255].repeat(8);
        let mut out = Vec::new();
        let mut raster = BitmapRaster::default();
        let mut cbdt = glyph(BitmapData::Bgra(&bgra), 2, 4, 20.0, Origin::TopLeft);
        cbdt.inner_bearing_x = 1.0;
        cbdt.inner_bearing_y = 5.0;
        assert_eq!(
            raster.render(&cbdt, 1000, 20.0, &mut out),
            Some(Placement {
                left: 1,
                top: 5,
                width: 2,
                height: 4
            })
        );

        let mut sbix = glyph(BitmapData::Bgra(&bgra), 2, 4, 20.0, Origin::BottomLeft);
        sbix.bearing_x = 100.0;
        sbix.bearing_y = -200.0;
        sbix.inner_bearing_x = 1.0;
        sbix.inner_bearing_y = 3.0;
        assert_eq!(
            raster.render(&sbix, 1000, 20.0, &mut out),
            Some(Placement {
                left: 3,
                top: 3,
                width: 2,
                height: 4
            })
        );
        assert_eq!(
            raster.render(&sbix, 1000, 40.0, &mut out),
            Some(Placement {
                left: 6,
                top: 6,
                width: 4,
                height: 8
            })
        );
    }
}
