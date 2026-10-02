//! The adversarial tree shapes the measure-cache bench times, shared with
//! the tests that pin what the cache retains for them: a deep chain and a
//! balanced broad tree.

use crate::layout::types::sizing::Sizing;
use crate::ui::Ui;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
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

pub(crate) fn build_broad(ui: &mut Ui) {
    build_broad_variant(ui, false);
}

pub(crate) fn build_broad_variant(ui: &mut Ui, changed: bool) {
    build_broad_level(ui, 0, 0, changed);
}

fn build_broad_level(ui: &mut Ui, depth: usize, key: usize, changed: bool) {
    Panel::vstack()
        .id_salt(("broad", depth, key))
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            if depth == BROAD_DEPTH {
                // The change is to layout authoring — one leaf's fill
                // weight, which an only child's geometry ignores. A colour
                // would be paint, which the measure cache does not key on,
                // and the whole tree would hit at the root.
                let weight = if changed && key == 0 { 2.0 } else { 1.0 };
                Block::new()
                    .id_salt(("broad-leaf", key))
                    .size((Sizing::fill(weight), Sizing::fixed(1.0)))
                    .show(ui);
                return;
            }

            for child in 0..BROAD_FANOUT {
                build_broad_level(ui, depth + 1, key * BROAD_FANOUT + child, changed);
            }
        });
}
