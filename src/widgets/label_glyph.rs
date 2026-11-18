//! [`Glyph`]: what a row paints before its label, in a text-line square of its own.

use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::shape::Shape;
use crate::shape::icon_paint::IconPaint;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// A glyph before a label: an icon of the registry, or shapes an app paints, in a leaf of its
/// own, so the row lays it out, a text line square, so it sits on the label's line, and handed the
/// label's style, so it takes the label's ink and changes with it.
pub(crate) trait Glyph: Sized {
    /// Whether it paints anything; a row records no leaf for one that does not.
    fn is_shown(&self) -> bool;

    /// Paints in the leaf's own space, a square of `text`'s line height from the origin.
    fn paint(self, ui: &mut Ui, text: &TextStyle);

    /// Records the glyph as leaf `id` before a label in `text`, while it shows.
    fn record(self, ui: &mut Ui, id: WidgetId, text: &TextStyle) {
        if !self.is_shown() {
            return;
        }
        let side = Sizing::fixed(text.line_height());
        Widget::leaf()
            .id(id)
            .size((side, side))
            .record(ui, None, |ui| self.paint(ui, text));
    }
}

/// An icon of the registry, tinted in the label's ink; `None` shows nothing.
impl Glyph for Option<IconHandle> {
    fn is_shown(&self) -> bool {
        self.is_some()
    }

    fn paint(self, ui: &mut Ui, text: &TextStyle) {
        if let Some(icon) = self {
            ui.add_shape(Shape::rect(), IconPaint::new(icon).tint(text.color));
        }
    }
}

/// Shapes an app paints, as [`RadioOption::show_with`](crate::RadioOption::show_with) takes them.
#[derive(Debug)]
pub(crate) struct Painted<F>(pub(crate) F);

impl<F: FnOnce(&mut Ui, &TextStyle)> Glyph for Painted<F> {
    fn is_shown(&self) -> bool {
        true
    }

    fn paint(self, ui: &mut Ui, text: &TextStyle) {
        (self.0)(ui, text);
    }
}
