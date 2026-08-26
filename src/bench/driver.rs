//! The driver registry: what a benchmark driver is, and every one the
//! crate has.

use crate::bench::{Arms, Run};
use crate::{animation, input, layout, primitives, renderer, scene, text, ui, widgets};
use criterion::Criterion;

/// One criterion driver, as [`run`](super::run) sees it.
///
/// The [`DRIVERS`] table is what makes selection *exact*: the runner
/// matches [`name`](Self::name) and only then calls [`run`](Self::run),
/// so a driver that wasn't asked for never executes its setup.
/// Criterion's own filter cannot do this — it gates the `bench_function`
/// call, and the setup happens before that.
#[derive(Debug)]
pub(super) struct Driver {
    /// Selection name, and the namespace every id this driver registers
    /// begins with (`damage/workload`, `frame/cached_cpu`). Unique across
    /// the table, and what `--driver` takes.
    ///
    /// A driver never spells this itself: it is handed down in
    /// [`Run::group`](super::Run::group), so renaming a row here renames
    /// its benchmarks with it and an id cannot end up under a namespace
    /// no row answers to.
    pub(super) name: &'static str,
    /// What this driver measures. Intersected with the run's request to
    /// decide whether it runs at all, and what it is handed.
    pub(super) arms: Arms,
    /// Kept out of the default set — the caller has to name it. For
    /// drivers whose cost or side effects make "ran it by accident" the
    /// wrong outcome: the frame matrix is ~90 s and appends a row to
    /// `benches/results/<machine>.txt` that demands a written note.
    ///
    /// Deliberately *not* folded into [`Self::arms`]: that says which
    /// hardware a driver exercises, this says whether running everything
    /// should include it. A cheap GPU driver would still be in the
    /// default set.
    pub(super) opt_in: bool,
    /// The driver's criterion configuration. Most want the default; the
    /// frame bench widens its measurement window (its GPU arms bounce
    /// ±15-25% across runs on a shared machine), which is why this is
    /// per-driver rather than one config for the whole run.
    pub(super) config: fn() -> Criterion,
    /// Runs the benchmarks against what the runner resolved. All but
    /// the frame bench ignore it.
    pub(super) run: fn(&mut Criterion, Run<'_>),
}

/// Every criterion driver in the crate.
///
/// **Hand-maintained**: a `bench.rs` whose `bench` fn has no row here is
/// invisible to the runner. The tests below pin the table's shape but
/// cannot see a function that was never added — the two edits belong in
/// one commit.
///
/// Sorted by `name` so `--list-drivers` and `--help` read in order.
pub(super) const DRIVERS: &[Driver] = &[
    driver("animation", animation::bench::bench),
    driver("caches", layout::cache::bench::bench),
    driver("cascade", scene::cascade::bench::bench),
    driver("composer", renderer::frontend::composer::bench::bench),
    gpu_driver(
        "curve_pipeline",
        renderer::backend::curve_pipeline::bench::bench,
    ),
    driver("damage", scene::damage::bench::bench),
    // The one row with both arms, and the reason `run` takes `Arms` at
    // all: `Cpu` executes zero GPU code while `Gpu` requests an adapter,
    // so it has to be told which was asked for. `opt_in` because the
    // full matrix is ~90 s and appends a noted results row.
    Driver {
        name: "frame",
        arms: Arms::Both,
        opt_in: true,
        config: ui::bench::config,
        run: ui::bench::bench,
    },
    driver("gradient", renderer::frontend::bench::bench),
    driver("gradient_atlas", renderer::gradient_atlas::bench::bench),
    driver("half_simd", primitives::half_simd::bench::bench),
    gpu_driver(
        "image_pipeline",
        renderer::backend::image_pipeline::bench::bench,
    ),
    driver("input", input::bench::bench),
    driver("paint_anims", scene::tree::paint_anims::bench::bench),
    gpu_driver("record_pass", renderer::backend::bench::bench),
    driver("schedule", renderer::backend::schedule::bench::bench),
    gpu_driver("text_atlas", renderer::backend::text::bench::bench),
    driver("text_edit", widgets::text_edit::bench::bench),
    driver(
        "text_grid",
        renderer::frontend::composer::text_grid::bench::bench,
    ),
    driver("text_shape", text::bench::bench),
];

/// A CPU driver on criterion's default configuration — the common row.
const fn driver(name: &'static str, run: fn(&mut Criterion, Run<'_>)) -> Driver {
    Driver {
        name,
        arms: Arms::Cpu,
        opt_in: false,
        config: Criterion::default,
        run,
    }
}

/// [`driver`] for one that requests an adapter.
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

    /// `--driver` matches on `name`, so a duplicate would make one row
    /// unreachable. Sorted because the table is also the `--help` and
    /// `--list-drivers` ordering, and an unsorted insert reads as
    /// arbitrary.
    #[test]
    fn driver_names_are_unique_and_sorted() {
        let names: Vec<&str> = DRIVERS.iter().map(|d| d.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "DRIVERS must be sorted by name");
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate driver name");
    }

    /// Which drivers touch a GPU, pinned by name. A driver that starts
    /// or stops needing a device changes what `--arms cpu` covers and
    /// whether a CPU profile stays clean, so the change should be
    /// deliberate rather than incidental.
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
        // `frame` is the only row with both, which is why `run` takes
        // the resolved arms at all.
        assert_eq!(
            DRIVERS
                .iter()
                .filter(|d| d.arms == Arms::Both)
                .map(|d| d.name)
                .collect::<Vec<_>>(),
            ["frame"],
        );
    }

    /// Only the frame bench is kept out of the default set, and it is
    /// the expensive one — a second opt-in row should be a deliberate
    /// call, not something that accumulates.
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
