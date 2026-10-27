//! The box-and-label boolean toggle and its pair of click responses.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::shape::style::{LineCap, LineJoin};
use crate::theme::Theme;
use crate::theme::toggle::ToggleTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::toggle_row::{Settled, ToggleKind, ToggleRow};

/// Boolean toggle over a `&mut bool` the owner controls (as egui). Clicking the row flips it.
///
/// An HStack of [box, label], one `Sense::CLICK` target; the parts are `id.with("box")` and `id.with("label")`, stable across sibling insertions. Visuals come from `theme.checkbox` ([`crate::ToggleTheme`]): chrome via the slot's `plan` (`unchecked` / `checked`), glyph color from `indicator`, geometry from `box_size`.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Checkbox<'a> {
    row: ToggleRow<'a>,
    value: &'a mut bool,
}

impl<'a> Checkbox<'a> {
    /// A box bound to `value`, which a click flips.
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        Self {
            row: ToggleRow::new(),
            value,
        }
    }

    /// The text drawn right of the box. Empty (default) records no text child.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.row.label = label.into();
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `checkbox`.
    pub fn style(mut self, s: impl Into<Option<&'a ToggleTheme>>) -> Self {
        self.row.style = s.into();
        self
    }

    /// Record the row and report whether this frame flipped the bool. A flip commits at once (`committed == changed`).
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        self.row.show(ui, Check(self.value))
    }
}

impl Configure for Checkbox<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.row.widget.configure()
    }
}

/// A square box with a check mark while set.
#[derive(Debug)]
struct Check<'a>(&'a mut bool);

impl ToggleKind for Check<'_> {
    fn settle(self, activated: bool) -> Settled {
        Settled::flip(self.0, activated)
    }

    fn slot(theme: &Theme) -> &ToggleTheme {
        &theme.checkbox
    }

    fn boxed(slot: &ToggleTheme) -> Widget {
        let side = Sizing::fixed(domain::length_at_least(slot.box_size, 1.0));
        Widget::leaf().size((side, side))
    }

    fn pill(_: &ToggleTheme) -> Option<f32> {
        None
    }

    fn paint(ui: &mut Ui, _: WidgetId, slot: &ToggleTheme, _: &Chrome, checked: bool) {
        if checked {
            let width = domain::length_at_least(slot.indicator_width, 0.0);
            ui.add_shape(
                Shape::polyline(&slot.check_polyline(), Stroke::new(slot.indicator, width))
                    .cap(LineCap::Round)
                    .join(LineJoin::Round),
            );
        }
    }
}

#[cfg(test)]
mod tests;
