//! The criterion benches, which `benches/palantir.rs` runs: `cargo drive bench`.
//!
//! Two kinds, and every case is one. An `atomic` case times one stage, one widget or one
//! operation on a made input. An `integration` case times palantir's frame, or a chain of its
//! stages from record onward, over a scene.
//!
//! A case's id is `<kind>/<group>/<backend>/<rest…>`, with the same depth across a group: the
//! backend, `cpu` or `gpu`, comes first in every case, so `'/cpu/'` selects every case that opens
//! no device and `'/gpu/'` every case that does. A GPU case runs only on a host with an adapter,
//! and a case that times one batch inside the pass only on an adapter that writes in-pass
//! timestamps; the device opens in the first GPU case a run selects. A case builds its inputs in
//! its own closure, so a filtered run builds only what it times, and checks what its name claims
//! there: a fixture that stops doing what its name says fails the case rather than timing
//! something else. Each case records one number: wall time, or, where the doc says so, the
//! device or pass time it reads back through `iter_custom`.

pub mod backends;
pub mod case_group;
pub(crate) mod fixture;
pub mod kind;

use crate::bench::backends::Backends;
use crate::bench::case_group::BetweenCases;
use crate::cascade;
use crate::gpu::backdrop;
use crate::gpu::backdrop::backdrop_blur;
use crate::gpu::frame::schedule;
use crate::gpu::pipeline::{curve_pipeline, image_pipeline};
use crate::gpu::raster::{raster_atlas, text_backend};
use crate::layout::cache as measure_cache;
use crate::primitives::packed::half_simd;
use crate::renderer::frontend;
use crate::renderer::frontend::composer;
use crate::renderer::gradient_atlas;
use crate::scene::tree::paint_animations;
use crate::widgets::{color_field, text_edit};
use crate::{animation, damage, gpu, input, layout, text, ui};
use criterion::Criterion;

/// Every case, atomic first. The host's adapters are listed once here; no device opens, as
/// [`BenchGpu::shared`](crate::gpu::bench_gpu::BenchGpu::shared) refuses to open one between
/// cases.
pub fn run(c: &mut Criterion) {
    let _between = BetweenCases::set(true);
    let backends = Backends::probe();

    animation::bench::atomic(c);
    backdrop::bench::atomic(c, backends);
    backdrop_blur::bench::atomic(c, backends);
    color_field::bench::atomic(c);
    composer::bench::atomic(c);
    curve_pipeline::bench::atomic(c, backends);
    damage::bench::atomic(c);
    frontend::bench::atomic(c);
    gradient_atlas::bench::atomic(c);
    half_simd::bench::atomic(c);
    cascade::bench::atomic(c);
    image_pipeline::bench::atomic(c, backends);
    input::bench::atomic(c);
    layout::bench::atomic(c);
    paint_animations::bench::atomic(c);
    gpu::bench::atomic(c, backends);
    gpu::resource::bench::atomic(c, backends);
    raster_atlas::bench::atomic(c, backends);
    schedule::bench::atomic(c);
    text_backend::bench::atomic(c, backends);
    text_edit::bench::atomic(c);
    text::bench::atomic(c);
    ui::bench::atomic(c, backends);

    backdrop::bench::integration(c, backends);
    damage::bench::integration(c);
    ui::bench::integration(c, backends);
    frontend::bench::integration(c);
    measure_cache::bench::integration(c);
}
