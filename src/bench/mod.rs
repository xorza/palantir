//! The benches, which `palantir-showcase/benches/palantir.rs` runs through [`main`]: the criterion
//! cases (`cargo bench-all`), or a study its first argument names (`cargo damage-sweep`,
//! `cargo atlas-memory`). One target, so one release link serves them all.
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
pub mod scene;
mod study;

use crate::bench::backends::Backends;
use crate::bench::case_group::BetweenCases;
use crate::bench::scene::Scene;
use crate::bench::study::{Invocation, Study};
use crate::cascade;
use crate::gpu::backdrop;
use crate::gpu::backdrop::backdrop_blur;
use crate::gpu::frame::schedule;
use crate::gpu::pipeline::{curve_pipeline, effect_pipeline, image_pipeline};
use crate::gpu::raster::text_backend;
use crate::layout::cache as measure_cache;
use crate::primitives::packed::half_simd;
use crate::renderer::cpu_gradient_atlas;
use crate::renderer::frontend;
use crate::renderer::frontend::composer;
use crate::renderer::render_buffer::rounded_clip;
use crate::scene::tree::paint_animations;
use crate::widgets::{color_field, text_edit};
use crate::{animation, damage, gpu, input, layout, text, ui};
use criterion::Criterion;
use std::env;
use std::process::ExitCode;

/// The bench target over `S`, as its command line says (`Invocation::of`): a study, every
/// criterion case, or with no argument at all, as `cargo bench-once` runs it, every case and then
/// every study once, so that run reaches all of them. Criterion reads its own arguments, as
/// `criterion_main!` lets it.
pub fn main<S: Scene>() -> ExitCode {
    let cases = || {
        let mut c = Criterion::default().configure_from_args();
        run::<S>(&mut c);
        c.final_summary();
    };
    match Invocation::of(env::args().skip(1)) {
        Invocation::Study(study, args) => study.run::<S>(args),
        Invocation::Cases => {
            cases();
            ExitCode::SUCCESS
        }
        Invocation::Everything => {
            cases();
            let failed = Study::ALL
                .into_iter()
                .map(|study| study.run::<S>(Vec::new()))
                .find(|&code| code != ExitCode::SUCCESS);
            failed.unwrap_or(ExitCode::SUCCESS)
        }
    }
}

/// Every case, atomic first, those that run a whole screen over `S`. The host's adapters are listed
/// once here; no device opens, as [`Backends::device`] refuses to open one between cases. The
/// devices drop with `backends`, when the run ends.
fn run<S: Scene>(c: &mut Criterion) {
    let _between = BetweenCases::set(true);
    let backends = Backends::probe();

    animation::bench::atomic(c);
    backdrop::bench::atomic::<S>(c, &backends);
    backdrop_blur::bench::atomic(c, &backends);
    color_field::bench::atomic(c);
    composer::bench::atomic(c);
    curve_pipeline::bench::atomic(c, &backends);
    damage::bench::atomic(c);
    effect_pipeline::bench::atomic(c, &backends);
    frontend::bench::atomic::<S>(c);
    cpu_gradient_atlas::bench::atomic(c);
    half_simd::bench::atomic(c);
    cascade::bench::atomic::<S>(c);
    image_pipeline::bench::atomic(c, &backends);
    input::bench::atomic(c);
    layout::bench::atomic::<S>(c);
    paint_animations::bench::atomic(c);
    gpu::bench::atomic(c, &backends);
    gpu::resource::bench::atomic(c, &backends);
    gpu::resource::shelf_packer::bench::atomic(c);
    rounded_clip::bench::atomic(c, &backends);
    schedule::bench::atomic(c);
    text_backend::bench::atomic(c, &backends);
    text_edit::bench::atomic(c);
    text::bench::atomic(c);
    ui::bench::atomic::<S>(c, &backends);

    backdrop::bench::integration::<S>(c, &backends);
    damage::bench::integration(c);
    damage::bench::policy::<S>(c, &backends);
    ui::bench::integration::<S>(c, &backends);
    rounded_clip::bench::integration(c, &backends);
    frontend::bench::integration(c);
    measure_cache::bench::integration(c);
}
