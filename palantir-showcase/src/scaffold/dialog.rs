//! [`Dialog`]: a modal dialog with a row of answers.

use palantir::{
    Configure, Justify, Modal, OverlayResponse, Panel, SizeSpec, Sizing, Text, TextWrap, Ui,
};

use crate::scaffold::kit::Kit;
use crate::scaffold::section_style;
use crate::scaffold::tokens::M;

/// A modal dialog: a title, a line of why, and a row of answers at the end, the confirming one
/// last, as macOS puts the default button at the trailing end.
#[derive(Debug)]
pub(crate) struct Dialog {
    pub(crate) title: &'static str,
    pub(crate) body: &'static str,
}

impl Dialog {
    /// `answers` records the buttons and closes through the handle.
    #[track_caller]
    pub(crate) fn show(
        self,
        ui: &mut Ui,
        k: Kit,
        answers: impl FnOnce(&mut Ui),
    ) -> OverlayResponse<()> {
        Modal::new().auto_id().show(ui, |ui| {
            Panel::vstack()
                .size((Sizing::fixed(DIALOG_W), Sizing::HUG))
                .gap(M.space.md)
                .show(ui, |ui| {
                    Text::new(self.title).style(&section_style(k)).show(ui);
                    Text::new(self.body)
                        .style(&k.t.dim())
                        .size(SizeSpec::FILL_W)
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui);
                    Panel::hstack()
                        .size(SizeSpec::FILL_W)
                        .margin((0.0, M.space.sm, 0.0, 0.0))
                        .gap(M.space.sm)
                        .justify(Justify::End)
                        .show(ui, |ui| answers(ui));
                });
        })
    }
}

/// A dialog's width: Fluent's ContentDialog keeps its body between 320 and 548 px.
const DIALOG_W: f32 = 360.0;
