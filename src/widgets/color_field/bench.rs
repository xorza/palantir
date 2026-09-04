//! What a hue drag costs: one field texture, filled.
//!
//! The field's texels are rebuilt on every frame the hue moves, and nothing
//! else in the picker is close to that cost. The question this answers is
//! whether the default divisor of 4 leaves the rebuild inside a frame — and
//! what dropping to 2 or 1 would cost if the accuracy at 4 is ever judged
//! short. See [`ColorField::downsample`](crate::ColorField::downsample) for
//! the error each divisor buys.
//!
//! Both models run, because they are not the same work: Okhsv solves the
//! gamut cusp once per field and then evaluates a rational map per texel,
//! while HSV is six comparisons and three multiplies.
//!
//! Texel counts are the real ones — a 208 × 160 logical field at display
//! scale 1.5 is 312 × 240 physical, which the divisor reduces from there.

use crate::bench::Run;
use crate::primitives::color::color_model::ColorModel;
use crate::widgets::color_field::fill;
use criterion::Criterion;
use glam::UVec2;
use std::hint::black_box;

/// The field's physical size on the machine this was written for: the themed
/// 208 × 160 at display scale 1.5.
const PHYSICAL: UVec2 = UVec2::new(312, 240);

fn texels(divisor: u32) -> UVec2 {
    UVec2::new(PHYSICAL.x / divisor, PHYSICAL.y / divisor)
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    let mut g = run.group(c);
    let mut buffer = Vec::new();
    for model in ColorModel::ALL {
        for divisor in [1u32, 2, 4] {
            let size = texels(divisor);
            let name = format!("fill/{}/divisor_{divisor}", model.label().to_lowercase());
            g.bench_function(&name, |b| {
                b.iter(|| {
                    buffer.clear();
                    fill(
                        &mut buffer,
                        black_box(size),
                        black_box(model),
                        black_box(0.6),
                    );
                    buffer.len()
                })
            });
        }
    }
}
