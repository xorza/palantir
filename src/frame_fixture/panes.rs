//! The split-pane card — the tree's only [`Splitter`].
//!
//! Deliberately the smallest honest carrier for one: a splitter is
//! `FILL`/`FILL`, so it needs a bounded box, and everything else in the
//! card column sits inside the page scroll, which passes ∞ on its main
//! axis. Hence the fixed height rather than a hug.
//!
//! The ratio is held constant across iterations like every other backing
//! value here — only `tick` moves — so the divider never perturbs the
//! steady-state damage the bench arms assert.

use crate::frame_fixture::FrameFixture;
use crate::frame_fixture::tokens;
use crate::layout::types::sizing::Sizing;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::splitter::{SplitHalf, Splitter};
use crate::widgets::text::Text;

pub(super) fn panes_card(state: &mut FrameFixture, ui: &mut Ui) {
    tokens::card(ui, "panes", "LAYOUT", Sizing::fixed(120.0), |ui| {
        Splitter::horizontal(&mut state.split)
            .id_salt("panes-split")
            .min_pane(80.0)
            .show(ui, |ui, half| {
                let (id, label) = match half {
                    SplitHalf::First => ("pane-a", "input"),
                    SplitHalf::Second => ("pane-b", "preview"),
                };
                Panel::vstack()
                    .id_salt(id)
                    .padding(8.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(tokens::well_bg())
                    .show(ui, |ui| {
                        Text::new(label)
                            .id_salt((id, "label"))
                            .style(&tokens::caption_style())
                            .show(ui);
                    });
            });
    });
}
