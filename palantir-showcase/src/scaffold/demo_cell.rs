//! [`DemoCell`]: a captioned square a paint demo sits in.

use palantir::{Configure, Panel, Sizing, Text, TextWrap, Ui};

use crate::scaffold::kit::Kit;
use crate::scaffold::tokens::M;
use crate::scaffold::{CAPTION_H, CELL_PADDING, TILE, light_well_chrome};

/// What a [`DemoCell`]'s body sits on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Surface {
    /// A padded well of the ground.
    Well,
    /// A padded [`LIGHT_WELL`](crate::scaffold::LIGHT_WELL), for drop shadows and dark strokes.
    LightWell,
    /// Nothing: the body fills the cell edge to edge.
    Bare,
}

/// A paint demo captioned under its tile, as a gallery labels a plate, [`TILE`] square on a
/// [`Surface::Well`] unless set otherwise, so tiles line up across pages. Keyed by its label, so a page can draw cells in a loop; cells in one row
/// need distinct labels.
#[derive(Debug)]
#[must_use]
pub(crate) struct DemoCell {
    label: &'static str,
    w: f32,
    h: f32,
    surface: Surface,
}

impl DemoCell {
    pub(crate) const fn new(label: &'static str) -> Self {
        Self {
            label,
            w: TILE,
            h: TILE,
            surface: Surface::Well,
        }
    }

    /// The cell's width and body height, for demos that don't fit a [`TILE`] square.
    pub(crate) const fn size(mut self, w: f32, h: f32) -> Self {
        self.w = w;
        self.h = h;
        self
    }

    pub(crate) const fn surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    pub(crate) fn show(self, ui: &mut Ui, k: Kit, body: impl FnOnce(&mut Ui)) {
        Panel::vstack()
            .id_salt(self.label)
            .size((Sizing::fixed(self.w), Sizing::HUG))
            .gap(M.space.sm)
            .show(ui, |ui| {
                let cell = Panel::zstack().size((Sizing::FILL, Sizing::fixed(self.h)));
                let cell = match self.surface {
                    Surface::Well => cell.padding(CELL_PADDING).chrome(k.t.card()),
                    Surface::LightWell => cell.padding(CELL_PADDING).chrome(light_well_chrome()),
                    Surface::Bare => cell,
                };
                cell.show(ui, body);
                Text::new(self.label)
                    .style(&k.t.caption())
                    .size((Sizing::FILL, Sizing::fixed(CAPTION_H)))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
            });
    }
}
