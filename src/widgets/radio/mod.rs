use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::rect::Rect;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::toggle::{ToggleChrome, toggle_row};
use crate::widgets::{Response, WidgetEntry, enter_widget};

/// One option in a radio group. `current` is the group's shared
/// selection; `value` is the option this row represents. Selected
/// when `*current == value`; clicking assigns `value` into `current`.
///
/// `T: PartialEq` is the only bound — works with any user enum,
/// tuple, or other equatable type. `value` is moved out on click, so
/// no `Clone` requirement.
///
/// Layout matches [`crate::Checkbox`]: HStack [pip, label], one
/// `Sense::CLICK` hit target spanning the whole row. Visuals come
/// from `theme.radio` ([`crate::ToggleTheme`]); the pip paints as a
/// pill (`box_size * 0.5` radius) regardless of `box_radius`.
pub struct RadioButton<'a, T: PartialEq> {
    element: Element,
    current: &'a mut T,
    value: T,
    label: InternedStr,
    style: Option<ToggleTheme>,
}

impl<'a, T: PartialEq> RadioButton<'a, T> {
    #[track_caller]
    pub fn new(current: &'a mut T, value: T) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.flags.set_sense(Sense::CLICK);
        Self {
            element,
            current,
            value,
            label: InternedStr::default(),
            style: None,
        }
    }

    pub fn label(mut self, s: impl Into<InternedStr>) -> Self {
        self.label = s.into();
        self
    }

    /// Override the theme for this radio button. `None` (default)
    /// inherits [`crate::Theme::radio`].
    pub fn style(mut self, s: ToggleTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: state,
        } = enter_widget(ui, &self.element);
        let mut selected = *self.current == self.value;
        // Radios latch — re-clicking the selected option is a no-op,
        // matches platform behavior on every OS. A fresh click selects
        // this option, so flip `selected` now (`value` is moved into
        // `current`, so we can't re-derive it) — otherwise the chrome +
        // pip below paint unselected until the next unrelated repaint.
        if state.left.clicked() && !state.disabled && !selected {
            *self.current = self.value;
            selected = true;
        }

        let theme = self.style.as_ref().unwrap_or(&ui.theme.radio);
        // `pill: true` forces the box chrome to a circle regardless of
        // any re-themed `radio.checked.normal.background.radius` — the
        // pip must never square-corner. Applied in `toggle_row`.
        let chrome = ToggleChrome::new(theme, state, selected, true);
        let indicator = theme.indicator;
        let dot_inset = theme.indicator_inset;

        toggle_row(
            ui,
            id,
            self.element,
            raw_state,
            chrome,
            self.label,
            |ui, pip_size| {
                if selected {
                    let dot_size = pip_size - 2.0 * dot_inset;
                    let dot = Rect::new(dot_inset, dot_inset, dot_size, dot_size);
                    ui.add_shape(
                        Shape::rect(dot)
                            .corners(Corners::all(dot_size * 0.5))
                            .fill(indicator),
                    );
                }
            },
        )
    }
}

impl<T: PartialEq> Configure for RadioButton<'_, T> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

#[cfg(test)]
mod tests;
