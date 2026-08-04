//! A viewer for the benchmark workload — the tree `cargo bench --bench
//! frame` records, drawn live at a window-sized scale.
//!
//! Deliberately the one page that builds none of its own content and
//! borrows none of [`crate::support`]'s tokens. The fixture's node
//! structure is what makes bench numbers comparable across releases, so
//! it owns its own look; a restyle of the tour must not reach in here and
//! silently retarget every recorded series.
//!
//! What to look at: it should read as a real app screen, not a pile of
//! widgets. Anything that looks broken here is a layout or paint
//! regression the timing numbers alone would not have caught.

use palantir::{FrameFixture, Ui};

/// Content multiplier. The benches use 32 against a 3840x6000 offscreen
/// target; this is sized so the card column fills a normal window instead
/// of running thousands of pixels past the bottom of the page scroll.
const SCALE: usize = 6;

pub(crate) fn build(ui: &mut Ui, fixture: &mut FrameFixture) {
    fixture.render(SCALE, ui);
}
