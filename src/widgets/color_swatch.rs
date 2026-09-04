//! A colour chip: the smallest thing that shows a colour, and the checker it
//! shows a translucent one against.

use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::RgbaF32;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;

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
        let mut node = Node::leaf();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            color,
            style: None,
        }
    }

    style_setter!('a, ColorPickerTheme, color_picker);

    /// Record the chip and report whether it was clicked.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut widget = ui.widget(self.node);
        let response = widget.response(ui);
        let id = widget.id();
        let theme = self.slot(ui.theme());
        let side = theme.swatch_size.themed_length(1.0);
        let checker = Checkerboard::new(theme, response.layout_rect, Size::new(side, side));
        let color = self.color;

        let node = &mut widget.node;
        node.size
            .get_or_insert((Sizing::fixed(side), Sizing::fixed(side)).into());
        widget.record(ui, None, |ui| checker.paint_chip(ui, color));
        Response::eager(id, ui, response)
    }
}

impl_configure!(ColorSwatch<'_>);
