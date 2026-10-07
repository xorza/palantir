//! The driver registry: what a benchmark driver is, and every one the crate has.

use crate::bench::{Arms, Run};
use crate::{
    animation, cascade, damage, gpu, input, layout, primitives, renderer, scene, text, ui, widgets,
};
use criterion::Criterion;

/// One criterion driver. The runner matches [`name`](Self::name) before
/// calling [`run`](Self::run), so an unselected driver never executes its setup.
#[derive(Debug)]
pub(super) struct Driver {
    /// Selection name (`--driver`) and the namespace of every id it registers;
    /// unique across the table.
    pub(super) name: &'static str,
    /// What this driver measures; intersected with the run's request.
    pub(super) arms: Arms,
    /// Kept out of the default set: the frame matrix takes ~90 s and appends a
    /// row to `benches/results/<machine>.txt` that needs a written note.
    pub(super) opt_in: bool,
    /// The criterion configuration; the frame bench widens its window, as its GPU arms bounce ±15-25%.
    pub(super) config: fn() -> Criterion,
    /// Runs the benchmarks against what the runner resolved.
    pub(super) run: fn(&mut Criterion, Run<'_>),
}

/// Every criterion driver, sorted by `name`. Hand-maintained: a `bench.rs`
/// with no row here is invisible.
pub(super) const DRIVERS: &[Driver] = &[
    driver("animation", animation::bench::bench),
    driver("caches", layout::cache::bench::bench),
    driver("cascade", cascade::bench::bench),
    driver("color_field", widgets::color_field::bench::bench),
    driver("composer", renderer::frontend::composer::bench::bench),
    gpu_driver(
        "curve_pipeline",
        gpu::pipeline::curve_pipeline::bench::bench,
    ),
    driver("damage", damage::bench::bench),
    // The one row with both arms, so `run` takes `Arms`; `opt_in` as the matrix takes ~90 s.
    Driver {
        name: "frame",
        arms: Arms::Both,
        opt_in: true,
        config: ui::bench::config,
        run: ui::bench::bench,
    },
    driver("gradient", renderer::frontend::bench::bench),
    driver("gradient_atlas", renderer::gradient_atlas::bench::bench),
    driver("half_simd", primitives::packed::half_simd::bench::bench),
    gpu_driver(
        "image_pipeline",
        gpu::pipeline::image_pipeline::bench::bench,
    ),
    driver("input", input::bench::bench),
    driver("paint_anims", scene::tree::paint_anims::bench::bench),
    gpu_driver("record_pass", gpu::bench::bench),
    driver(
        "rect_grid",
        renderer::frontend::composer::rect_grid::bench::bench,
    ),
    driver("schedule", gpu::frame::schedule::bench::bench),
    gpu_driver("text_atlas", gpu::raster::text_backend::bench::bench),
    driver("text_edit", widgets::text_edit::bench::bench),
    driver("text_shape", text::bench::bench),
];

const fn driver(name: &'static str, run: fn(&mut Criterion, Run<'_>)) -> Driver {
    Driver {
        name,
        arms: Arms::Cpu,
        opt_in: false,
        config: Criterion::default,
        run,
    }
}

const fn gpu_driver(name: &'static str, run: fn(&mut Criterion, Run<'_>)) -> Driver {
    Driver {
        arms: Arms::Gpu,
        ..driver(name, run)
    }
}

#[cfg(test)]
mod tests {
    use crate::bench::Arms;
    use crate::bench::driver::DRIVERS;

    /// A duplicate `name` would make a row unreachable; sorted for `--help` order.
    #[test]
    fn driver_names_are_unique_and_sorted() {
        let names: Vec<&str> = DRIVERS.iter().map(|d| d.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "DRIVERS must be sorted by name");
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate driver name");
    }

    /// Which drivers touch a GPU, pinned by name since it changes what `--arms cpu` covers.
    #[test]
    fn gpu_arms_are_the_expected_drivers() {
        let gpu: Vec<&str> = DRIVERS
            .iter()
            .filter(|d| d.arms.includes_gpu())
            .map(|d| d.name)
            .collect();
        assert_eq!(
            gpu,
            [
                "curve_pipeline",
                "frame",
                "image_pipeline",
                "record_pass",
                "text_atlas"
            ],
        );
        assert_eq!(
            DRIVERS
                .iter()
                .filter(|d| d.arms == Arms::Both)
                .map(|d| d.name)
                .collect::<Vec<_>>(),
            ["frame"],
        );
    }

    /// Only the expensive frame bench is opt-in.
    #[test]
    fn frame_is_the_only_opt_in_driver() {
        let opt_in: Vec<&str> = DRIVERS
            .iter()
            .filter(|d| d.opt_in)
            .map(|d| d.name)
            .collect();
        assert_eq!(opt_in, ["frame"]);
    }
}
