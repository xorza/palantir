//! The final render's deliveries: linear frames averaged, shrunk by an area filter in linear
//! light and encoded to sRGB once, so one render feeds every delivery. Refused: ffmpeg's scaling
//! of encoded sRGB, which darkens thin light text on a dark ground (Brasseur, "Gamma error in
//! picture scaling"); averaging 8-bit sRGB sub-frames, which quantizes each and darkens a
//! light-dark edge.

use half::f16;
use palantir::{RgbaF32, SrgbaU8};

/// A frame of linear RGBA in `f32`, rows top down.
#[derive(Debug, Default)]
pub(crate) struct Linear {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) texels: Vec<[f32; 4]>,
}

impl Linear {
    /// `bytes`, an `Rgba16Float` capture of `width × height`, into this frame.
    pub(crate) fn load(&mut self, bytes: &[u8], width: usize, height: usize) {
        assert_eq!(bytes.len(), width * height * 8, "an Rgba16Float frame");
        self.width = width;
        self.height = height;
        self.texels.clear();
        self.texels
            .extend(bytes.as_chunks::<8>().0.iter().map(|texel| {
                [0, 2, 4, 6].map(|at| f16::from_le_bytes([texel[at], texel[at + 1]]).to_f32())
            }));
    }

    /// The mean of this frame and `other`, texel by texel, kept here: two sub-frames of a 180°
    /// shutter blended in linear light.
    pub(crate) fn average(&mut self, other: &Self) {
        assert_eq!((self.width, self.height), (other.width, other.height));
        for (a, b) in self.texels.iter_mut().zip(&other.texels) {
            *a = [0, 1, 2, 3].map(|c| f32::midpoint(a[c], b[c]));
        }
    }
}

/// An area filter from one size to a smaller one: each output texel the mean of the input it
/// covers, a texel cut by its edge weighed by the share it covers.
#[derive(Debug)]
pub(crate) struct Shrink {
    width: usize,
    height: usize,
    columns: Vec<Taps>,
    rows: Vec<Taps>,
    across: Vec<[f32; 4]>,
}

/// The input texels one output texel covers on one axis, and their weights, summing to 1.
#[derive(Clone, Debug)]
struct Taps {
    first: usize,
    weights: Vec<f32>,
}

/// The taps of each of `to` outputs over `from` inputs: output `i` covers `[i · r, (i + 1) · r)`
/// with `r = from / to`, computed on integer multiples of `to` so a 3:2 cut lands exactly.
fn taps(from: usize, to: usize) -> Vec<Taps> {
    assert!(to > 0 && to <= from, "a shrink from {from} to {to}");
    (0..to)
        .map(|i| {
            let (start, end) = (i * from, (i + 1) * from);
            let first = start / to;
            let last = (end - 1) / to;
            let weights = (first..=last)
                .map(|j| {
                    let lo = (j * to).max(start);
                    let hi = ((j + 1) * to).min(end);
                    (hi - lo) as f32 / from as f32
                })
                .collect();
            Taps { first, weights }
        })
        .collect()
}

impl Shrink {
    pub(crate) fn new(from: (usize, usize), to: (usize, usize)) -> Self {
        Self {
            width: to.0,
            height: to.1,
            columns: taps(from.0, to.0),
            rows: taps(from.1, to.1),
            across: Vec::new(),
        }
    }

    /// `frame` shrunk and encoded into `rgba`, 8-bit sRGB RGBA rows top down.
    pub(crate) fn encode(&mut self, frame: &Linear, rgba: &mut Vec<u8>) {
        self.across.clear();
        self.across.reserve_exact(self.width * frame.height);
        for row in frame.texels.chunks_exact(frame.width) {
            for taps in &self.columns {
                self.across.push(blend(row, taps, 1, 0));
            }
        }
        rgba.clear();
        rgba.reserve_exact(self.width * self.height * 4);
        for taps in &self.rows {
            for x in 0..self.width {
                let [r, g, b, a] = blend(&self.across, taps, self.width, x);
                let SrgbaU8 { r, g, b, a } = RgbaF32::new(r, g, b, a).to_srgba8();
                rgba.extend([r, g, b, a]);
            }
        }
    }
}

/// The weighted mean of `taps` over `texels`, the `k`-th tap at `offset + (first + k) · stride`.
fn blend(texels: &[[f32; 4]], taps: &Taps, stride: usize, offset: usize) -> [f32; 4] {
    let mut sum = [0.0; 4];
    for (k, weight) in taps.weights.iter().enumerate() {
        let texel = texels[offset + (taps.first + k) * stride];
        for (total, value) in sum.iter_mut().zip(texel) {
            *total += weight * value;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use palantir::RgbaF32;

    use crate::downscale::{Linear, Shrink, taps};

    /// 2:1 takes two whole texels at half each; 3:2 takes a whole texel and half the next at
    /// 1/1.5 and 0.5/1.5 for an even output, the mirror for an odd one.
    #[test]
    fn taps_weigh_each_texel_by_the_share_it_covers() {
        let two = taps(4, 2);
        assert_eq!((two[1].first, two[1].weights.clone()), (2, vec![0.5, 0.5]));
        let three = taps(3, 2);
        assert_eq!(
            (three[0].first, three[0].weights.clone()),
            (0, vec![2.0 / 3.0, 1.0 / 3.0])
        );
        assert_eq!(
            (three[1].first, three[1].weights.clone()),
            (1, vec![1.0 / 3.0, 2.0 / 3.0])
        );
    }

    /// A 2 × 2 block of linear 0, 1, 1, 0 shrinks 2:1 to linear 0.5, sRGB code 188
    /// (`1.055 · 0.5^(1/2.4) − 0.055 = 0.7354`, × 255 = 187.5, rounds to 188); in encoded sRGB
    /// the mean would be code 128, darker. A 3 × 1 row of 0, 1, 0 shrinks 3:2 to
    /// `(2·0 + 1·1)/3 = 1/3` and its mirror, code 156 each (`1.055 · (1/3)^(1/2.4) − 0.055 =
    /// 0.6125`, × 255 = 156.2).
    #[test]
    fn a_block_shrinks_to_its_linear_mean() {
        let frame = |width, height, values: &[f32]| Linear {
            width,
            height,
            texels: values.iter().map(|&v| [v, v, v, 1.0]).collect(),
        };
        let mut rgba = Vec::new();
        Shrink::new((2, 2), (1, 1)).encode(&frame(2, 2, &[0.0, 1.0, 1.0, 0.0]), &mut rgba);
        assert_eq!(rgba, [188, 188, 188, 255]);
        assert_eq!(RgbaF32::new(0.5, 0.5, 0.5, 1.0).to_srgba8().r, 188);
        Shrink::new((3, 1), (2, 1)).encode(&frame(3, 1, &[0.0, 1.0, 0.0]), &mut rgba);
        let third = 1.0 / 3.0;
        assert_eq!(RgbaF32::new(third, third, third, 1.0).to_srgba8().r, 156);
        assert_eq!(rgba, [156, 156, 156, 255, 156, 156, 156, 255]);
    }
}
