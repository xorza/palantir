//! What a hue drag costs: one field texture, filled. Texels rebuild on every hue-moving frame; answers whether the default divisor of 4 fits a frame (see [`ColorField::texel_size`](crate::ColorField::texel_size)). Both models run: Okhsv solves the gamut cusp once per field, HSV is six comparisons and three multiplies. A 208 × 160 field at scale 1.5 is 312 × 240 physical, reduced by the divisor.

use crate::bench::Run;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::image::Image;
use crate::widgets::color_field::fill;
use criterion::Criterion;
use glam::UVec2;
use std::hint::black_box;

const PHYSICAL: UVec2 = UVec2::new(312, 240);

const fn size_at(divisor: u32) -> UVec2 {
    UVec2::new(PHYSICAL.x / divisor, PHYSICAL.y / divisor)
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut g = run.group(c);
    for model in ColorModel::ALL {
        // The default divisor, and a texel per pixel as the worst a caller can set.
        for divisor in [1u32, 4] {
            let mut image = Image::blank(size_at(divisor));
            let name = format!("fill/{}/divisor_{divisor}", model.label().to_lowercase());
            g.bench_function(&name, |b| {
                b.iter(|| {
                    fill(&mut image, black_box(model), black_box(0.6));
                    image.texels().len()
                });
            });
        }
    }
}
