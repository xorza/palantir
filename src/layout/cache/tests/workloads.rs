//! What the cache retains for the bench's adversarial trees, and which subtrees a localized change still hits.

use crate::internals::harness::UiHarness;
use crate::layout::cache::internals::{
    BROAD_DEPTH, BROAD_FANOUT, BroadChange, DEEP_DEPTH, build_broad, build_broad_variant,
    build_deep,
};
use crate::scene::layer::Layer;
use crate::ui::Ui;

fn first_frame(build: fn(&mut Ui)) -> UiHarness {
    let mut h = UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0);
    let _ = h.frame(build);
    h
}

#[test]
fn adversarial_workloads_retain_one_row_per_node() {
    let deep = first_frame(build_deep);
    let deep_nodes = DEEP_DEPTH + 2;
    assert_eq!(
        deep.ui.tree(Layer::Main).records.len(),
        deep_nodes,
        "viewport + {DEEP_DEPTH} nested panels + leaf",
    );
    assert_eq!(
        deep.engines.layout.cache.captured_desired().len(),
        deep_nodes,
        "deep trees retain one row per node",
    );

    let broad = first_frame(build_broad);
    let panel_count = (0..=BROAD_DEPTH)
        .map(|depth| BROAD_FANOUT.pow(depth as u32))
        .sum::<usize>();
    let leaf_count = BROAD_FANOUT.pow(BROAD_DEPTH as u32);
    assert_eq!(
        broad.ui.tree(Layer::Main).records.len(),
        1 + panel_count + leaf_count,
        "viewport + balanced panels + one leaf per terminal panel",
    );
    let broad_nodes = 1 + panel_count + leaf_count;
    assert_eq!(
        broad.engines.layout.cache.captured_desired().len(),
        broad_nodes,
        "balanced trees retain one row per node",
    );
}

/// One changed leaf re-measures the panels above it; every sibling subtree beside the path hits (seven at each of three levels), even though a taller leaf raises each panel's minimum offer, since a Hug stack of fixed leaves holds under any larger offer.
#[test]
fn localized_change_hits_unchanged_sibling_subtrees() {
    for change in [BroadChange::FillWeight, BroadChange::LeafHeight] {
        let mut h = UiHarness::new(glam::UVec2::new(1280, 800)).scale(2.0);
        let _ = h.frame(|ui| build_broad_variant(ui, None));
        let _ = h.frame(|ui| build_broad_variant(ui, Some(change)));
        assert_eq!(
            h.engines.layout.scratch.counters.cache_hits().len(),
            21,
            "{change:?}: seven unchanged siblings hit at each of the three branch levels",
        );
    }
}
