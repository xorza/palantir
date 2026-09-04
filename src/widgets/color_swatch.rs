//! A colour chip: the smallest thing that shows a colour, and the checker it
//! shows a translucent one against.

use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::RgbaF32;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use std::rc::Rc;

/// A chip painting one colour, with a checkerboard behind it when that colour
/// is translucent.
///
/// The display half of the colour family: it writes nothing and senses only a
/// click, so a caller builds a preset row, a recent-colours strip or a
/// "before / after" pair out of it and reads
/// [`clicked`](crate::Response::clicked) itself.
///
/// Sized from [`ColorPickerTheme::swatch_size`], and styled from the same
/// bundle as the rest of the family.
#[derive(Debug)]
pub struct ColorSwatch<'a> {
    node: Node,
    color: RgbaF32,
    style: Option<&'a ColorPickerTheme>,
}

impl<'a> ColorSwatch<'a> {
    /// A chip showing `color`.
    #[track_caller]
    pub fn new(color: RgbaF32) -> Self {
        Self {
            node: Node::leaf().sense(Sense::CLICK),
            color,
            style: None,
        }
    }

    style_setter!('a, ColorPickerTheme, color_picker);

    /// Record the chip and report whether it was clicked.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        // The theme handle is cloned so the slot outlives the `&mut Ui` the
        // widget opening below takes: the checker reads it after that.
        let bundle = Rc::clone(ui.theme());
        let theme = self.slot(&bundle);
        let side = theme.swatch_size.themed_length(1.0);
        let node = self
            .node
            .default_size((Sizing::fixed(side), Sizing::fixed(side)));
        let widget = ui.widget(node);
        let response = widget.response(ui);
        let id = widget.id();
        let checker = Checkerboard::new(theme, response.layout_rect, Size::new(side, side));
        let color = self.color;

        widget.record(ui, None, |ui| checker.paint_chip(ui, color));
        Response::eager(id, ui, response)
    }
}

impl_configure!(ColorSwatch<'_>);
