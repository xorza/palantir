//! A viewer for the benchmark workload (the tree `cargo bench -p palantir --bench criterion -- -d frame` records) drawn live at window scale. `frame` is the one opt-in driver, run only when `--driver` names it.
//!
//! The one page that builds no content of its own and borrows none of [`crate::support`]'s tokens: the fixture's node structure makes bench numbers comparable across releases, so a tour restyle must not retarget recorded series.
//!
//! It should read as a real app screen; anything broken here is a layout or paint regression timings alone would miss.

use palantir::internals::frame_fixture::FrameFixture;
use palantir::{Ui, WidgetId};

/// Content multiplier. The benches use 32 (`BENCH_SCALE`); this fills a normal window.
const SCALE: usize = 6;

/// The fixture holds the values the tree binds `&mut` to, so it lives in a state row as in the benches; a fresh one each frame would reset every control.
pub(crate) fn build(ui: &mut Ui) {
    let id = WidgetId::from_hash("showcase::frame_bench::fixture");
    ui.with_state::<FrameFixture, _>(id, |ui, fixture| fixture.render(SCALE, ui));
}
