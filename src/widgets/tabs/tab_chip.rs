//! What a chip's content is handed: the tab it stands for, and the ink its look wears this frame.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::text::interned_str::InternedStr;
use crate::text::wrap::TextWrap;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::text::Text;

/// One chip, as [`TabStrip::show`](crate::TabStrip::show) hands it to the content it records
/// between the chip's padding and its close button. The strip keeps the frame, the selection cap,
/// the close button, the senses and the keyboard; the content is only what the tab shows.
#[derive(Clone, Copy, Debug)]
pub struct TabChip {
    /// The chip's index in the strip's items.
    pub slot: usize,
    /// The tab's [`TabItem::key`](crate::TabItem::key).
    pub key: u64,
    /// The tab's [`TabItem::label`](crate::TabItem::label).
    pub label: InternedStr,
    /// Whether the chip wears the selection cap.
    pub selected: bool,
    /// The chip look's text, animated with its fill: content in this ink follows hover and selection.
    pub text: TextStyle,
    /// The chip's id, for the content's own ids, as `chip.id.with("icon")`. The strip takes
    /// `chip.id.with("content")` for the box the content records in and `chip.id.with("cap")`
    /// for the selection cap, and [`Self::show_label`] takes `chip.id.with("label")`.
    pub id: WidgetId,
}

impl TabChip {
    /// Records the label in the look's ink, ellipsized at
    /// [`TabsTheme::max_width`](crate::TabsTheme::max_width): the content a chip shows by default.
    pub fn show_label(&self, ui: &mut Ui) {
        Text::new(self.label)
            .id(self.id.with("label"))
            .style(&self.text)
            .text_wrap(TextWrap::Ellipsis)
            .show(ui);
    }
}
