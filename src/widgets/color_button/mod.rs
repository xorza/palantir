//! The chip that opens a picker: a swatch-styled trigger, and the popup it
//! drops.

use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::num::F32Ext;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::popup::Popup;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::value_response::ValueResponse;
use std::rc::Rc;

/// A colour chip that opens a [`ColorPicker`] in a popup when clicked.
///
/// The compact form of the picker, for a properties panel or a node's port:
/// one chip the size of a preview, and the panel only while it is wanted.
/// Open state lives in the response map keyed off the trigger, so a caller
/// threads nothing but the colour.
///
/// Clicking outside or pressing Esc closes it. There is no revert, because
/// every gesture inside the panel has already committed — the chip shows what
/// the colour is, not a proposal.
#[derive(Debug)]
pub struct ColorButton<'a> {
    node: Node,
    color: &'a mut RgbaF32,
    alpha: bool,
    model: Option<ColorModel>,
    history: bool,
    style: Option<&'a ColorPickerTheme>,
}

/// Open/closed flag for one trigger site, keyed off the trigger id.
#[derive(Default, Clone, Copy, Debug)]
struct ChipState {
    open: bool,
}

impl<'a> ColorButton<'a> {
    /// A chip bound to `color`.
    #[track_caller]
    pub fn new(color: &'a mut RgbaF32) -> Self {
        Self {
            node: Node::leaf().sense(Sense::CLICK),
            color,
            alpha: false,
            model: None,
            history: true,
            style: None,
        }
    }

    /// Show the alpha bar and the opacity value in the popup. Off by default,
    /// matching [`ColorPicker::alpha`].
    pub fn alpha(mut self, on: bool) -> Self {
        self.alpha = on;
        self
    }

    /// Pin the popup's model instead of offering the switch.
    pub fn model(mut self, model: ColorModel) -> Self {
        self.model = Some(model);
        self
    }

    /// Show the picker's own swatch row. On by default: a chip in a panel is
    /// the case with no room for a preset row of its own.
    pub fn history(mut self, on: bool) -> Self {
        self.history = on;
        self
    }

    style_setter!('a, ColorPickerTheme, color_picker);

    /// Record the chip, and the popup when it is open.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        // The theme handle is cloned so the slot outlives the `&mut Ui` the
        // widget opening below takes: the checker reads it after that.
        let bundle = Rc::clone(ui.theme());
        let theme = self.slot(&bundle);
        let side = theme.chip_size.themed_length(1.0);
        let node = self
            .node
            .default_size((Sizing::fixed(side), Sizing::fixed(side)));
        let widget = ui.widget(node);
        let response = widget.response(ui);
        let id = widget.id();
        let checker = Checkerboard::new(theme, response.layout_rect, (side, side).into());
        let color = self.color;
        let shown = *color;

        widget.record(ui, None, |ui| checker.paint_chip(ui, shown));

        // Probed, not inserted: a chip spends nearly every frame closed, and
        // closed is the default — so an unopened trigger keeps no row at all.
        let was_open = ui
            .try_state::<ChipState>(id)
            .is_some_and(|state| state.open);
        let mut open = was_open;
        if !response.disabled && response.left.clicked() {
            open = !open;
        }

        let mut changed = false;
        let mut committed = false;
        if open && let Some(rect) = response.rect {
            let alpha = self.alpha;
            let model = self.model;
            let history = self.history;
            let popup = Popup::below(rect).id(id.with("panel"));
            let opened = popup.show(ui, |ui, _| {
                let mut picker = ColorPicker::new(color).alpha(alpha).history(history);
                if let Some(model) = model {
                    picker = picker.model(model);
                }
                let r = picker.id(id.with("picker")).show(ui);
                (r.changed, r.committed)
            });
            (changed, committed) = opened.inner;
            if opened.closed() {
                open = false;
            }
        }
        if open != was_open {
            ui.state_mut::<ChipState>(id).open = open;
        }

        ValueResponse {
            response: Response::eager(id, ui, response),
            changed,
            committed,
        }
    }
}

impl_configure!(ColorButton<'_>);

#[cfg(test)]
mod tests;
