//! [`LabelIcon`]: an icon a row lays out before its label, in the label's ink.

use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::shape::Shape;
use crate::shape::icon_paint::IconPaint;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// An icon beside a label: a leaf of its own, so the row lays it out, a text line square and
/// tinted in the label's ink, so it sits on the label's line and changes ink with it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LabelIcon {
    pub(crate) id: WidgetId,
    pub(crate) icon: IconHandle,
}

impl LabelIcon {
    /// Records the icon for a label in `text`.
    pub(crate) fn show(self, ui: &mut Ui, text: &TextStyle) {
        let side = Sizing::fixed(text.line_height());
        let ink = text.color;
        Widget::leaf()
            .id(self.id)
            .size((side, side))
            .record(ui, None, |ui| {
                ui.add_shape(Shape::rect(), IconPaint::new(self.icon).tint(ink));
            });
    }
}
