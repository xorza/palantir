//! The two trees the measure-cache benches and tests walk: a deep chain of panels and a broad
//! fan-out, one leaf of which a variant changes.

use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;

/// Nested panels in the deep chain, above its one leaf.
pub(crate) const DEEP_DEPTH: usize = 192;
/// Children per panel in the broad tree.
pub(crate) const BROAD_FANOUT: usize = 8;
/// Panel levels below the broad tree's root.
pub(crate) const BROAD_DEPTH: usize = 3;

pub(crate) fn build_deep(ui: &mut Ui) {
    build_deep_level(ui, 0);
}

fn build_deep_level(ui: &mut Ui, depth: usize) {
    if depth == DEEP_DEPTH {
        Block::new()
            .id_salt("deep-leaf")
            .size((Sizing::FILL, Sizing::fixed(1.0)))
            .show(ui);
        return;
    }

    Panel::vstack()
        .id_salt(("deep", depth))
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| build_deep_level(ui, depth + 1));
}

/// What a variant of the broad tree changes in its first leaf. Both are
/// layout authoring; a color is paint, which the cache does not key on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BroadChange {
    /// The leaf's fill weight, which an only child's geometry ignores.
    FillWeight,
    /// The leaf's height, which moves the minimum of every panel above it.
    LeafHeight,
}

pub(crate) fn build_broad(ui: &mut Ui) {
    build_broad_variant(ui, None);
}

pub(crate) fn build_broad_variant(ui: &mut Ui, change: Option<BroadChange>) {
    BroadLevel {
        depth: 0,
        key: 0,
        change,
    }
    .show(ui);
}

/// One panel of the broad tree: `depth` levels below the root, `key` its
/// pre-order index among its level's panels.
#[derive(Clone, Copy, Debug)]
struct BroadLevel {
    depth: usize,
    key: usize,
    change: Option<BroadChange>,
}

impl BroadLevel {
    fn show(self, ui: &mut Ui) {
        let Self { depth, key, change } = self;
        Panel::vstack()
            .id_salt(("broad", depth, key))
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                if depth == BROAD_DEPTH {
                    let changed = |to| key == 0 && change == Some(to);
                    let weight = if changed(BroadChange::FillWeight) {
                        2.0
                    } else {
                        1.0
                    };
                    let height = if changed(BroadChange::LeafHeight) {
                        2.0
                    } else {
                        1.0
                    };
                    Block::new()
                        .id_salt(("broad-leaf", key))
                        .size((Sizing::fill(weight), Sizing::fixed(height)))
                        .show(ui);
                    return;
                }

                for child in 0..BROAD_FANOUT {
                    Self {
                        depth: depth + 1,
                        key: key * BROAD_FANOUT + child,
                        change,
                    }
                    .show(ui);
                }
            });
    }
}
