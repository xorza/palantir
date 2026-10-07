//! The split-pane card, the tree's only [`Splitter`].
//!
//! A splitter is `FILL`/`FILL` and needs a bounded box, but the rest of the card column sits in the page scroll, which passes ∞ on its main axis; hence a fixed height, not a hug.
//!
//! The ratio stays constant across iterations (only `tick` moves) so the divider never perturbs the steady-state damage the bench arms assert.

use crate::internals::frame_fixture::FrameFixture;
use crate::internals::frame_fixture::tokens;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::splitter::Splitter;
use crate::widgets::splitter::split_half::SplitHalf;
use crate::widgets::text::Text;

pub(super) fn panes_card(state: &mut FrameFixture, ui: &mut Ui) {
    tokens::card(ui, "panes", "LAYOUT", Sizing::fixed(120.0), |ui| {
        Splitter::row(&mut state.split)
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
