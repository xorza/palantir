//! The titled card every section of the fixture sits in.

use crate::internals::frame_fixture::tokens;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;

/// Titled card: caption over its body on [`tokens::card_chrome`].
#[derive(Debug)]
pub(super) struct Card {
    id: &'static str,
    title: &'static str,
    height: Sizing,
}

impl Card {
    /// A card hugging its content.
    pub(super) const fn new(id: &'static str, title: &'static str) -> Self {
        Self {
            id,
            title,
            height: Sizing::HUG,
        }
    }

    /// The card's height, for the two that must not grow with content.
    pub(super) const fn height(mut self, height: Sizing) -> Self {
        self.height = height;
        self
    }

    pub(super) fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
        let Self { id, title, height } = self;
        Panel::vstack()
            .id_salt(id)
            .gap(8.0)
            .padding(10.0)
            .size((Sizing::FILL, height))
            .chrome(tokens::card_chrome())
            .show(ui, |ui| {
                Text::new(title)
                    .id_salt((id, "section"))
                    .style(&tokens::section_style())
                    .show(ui);
                body(ui);
            });
    }
}
