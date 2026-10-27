//! One option of a radio group, over the shared value the whole group writes through.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::theme::Theme;
use crate::theme::toggle::ToggleTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::toggle_row::{Settled, ToggleKind, ToggleRow};

/// One option in a radio group: selected when `*cur == value`, and clicking assigns `value` into
/// `cur`. [`Self::show`] needs `T: PartialEq`, the only bound. Layout and parts match
/// [`crate::Checkbox`] (pip, label); the pip is always a pill. Visuals come from `theme.radio` ([`crate::ToggleTheme`]).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct RadioButton<'a, T> {
    row: ToggleRow<'a>,
    cur: &'a mut T,
    value: T,
}

impl<'a, T> RadioButton<'a, T> {
    /// Writes `value` into `cur` when picked; selected while the two are equal.
    #[track_caller]
    pub fn new(cur: &'a mut T, value: T) -> Self {
        Self {
            row: ToggleRow::new(),
            cur,
            value,
        }
    }

    /// Label text.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.row.label = label.into();
        self
    }

    /// Theme slot.
    pub fn style(mut self, s: impl Into<Option<&'a ToggleTheme>>) -> Self {
        self.row.style = s.into();
        self
    }
}

impl<T: PartialEq> RadioButton<'_, T> {
    /// Records the row. A radio latches: `response.clicked()` is true on the already-selected option but
    /// `changed` is not. A pick commits at once, so `committed == changed`.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        self.row.show(
            ui,
            Pip {
                cur: self.cur,
                value: self.value,
            },
        )
    }
}

impl<T> Configure for RadioButton<'_, T> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.row.widget.configure()
    }
}

/// A round box with a dot while selected.
#[derive(Debug)]
struct Pip<'a, T> {
    cur: &'a mut T,
    value: T,
}

impl<T: PartialEq> ToggleKind for Pip<'_, T> {
    /// Radios latch: picking the selected option moves nothing.
    fn settle(self, activated: bool) -> Settled {
        let selected = *self.cur == self.value;
        let changed = activated && !selected;
        if changed {
            *self.cur = self.value;
        }
        Settled {
            checked: selected || changed,
            changed,
        }
    }

    fn slot(theme: &Theme) -> &ToggleTheme {
        &theme.radio
    }

    fn boxed(slot: &ToggleTheme) -> Widget {
        let side = Sizing::fixed(domain::length_at_least(slot.box_size, 1.0));
        Widget::leaf().size((side, side))
    }

    fn pill(slot: &ToggleTheme) -> Option<f32> {
        Some(domain::length_at_least(slot.box_size, 1.0) * 0.5)
    }

    fn paint(ui: &mut Ui, _: WidgetId, slot: &ToggleTheme, _: &Chrome, checked: bool) {
        if checked {
            let pip = domain::length_at_least(slot.box_size, 1.0);
            let inset = domain::length_at_least(slot.indicator_inset, 0.0);
            let dot = domain::length_at_least(pip - 2.0 * inset, 0.0);
            ui.add_shape(
                Shape::rect(Rect::new(inset, inset, dot, dot))
                    .corners(dot * 0.5)
                    .fill(slot.indicator),
            );
        }
    }
}

#[cfg(test)]
mod tests;
