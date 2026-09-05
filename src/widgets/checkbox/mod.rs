//! The box-and-label boolean toggle, and the pair of responses a click
//! on either half reports.

use crate::layout::types::sizing::Sizing;
use crate::primitives::num::F32Ext;
use crate::primitives::text_input::TextInput;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::response::Response;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::theme::widget_look::theme_slot::ThemeSlot;
use crate::widgets::toggle_chrome::ToggleChrome;
use crate::widgets::widget::Widget;

/// Two-response boolean toggle. Takes a `&mut bool` whose owner controls
/// the value — same pattern as egui. Clicking the row flips it.
///
/// Layout: HStack [box, label]. The whole row is one hit target with
/// `Sense::CLICK`; clicking anywhere on it toggles. Child node ids
/// derive from the outer widget id via `WidgetId::with`, so they stay
/// stable across sibling insertions (no reliance on `SeenIds`'
/// occurrence-counter disambiguation).
///
/// Visuals come from `theme.checkbox` ([`crate::ToggleTheme`]) — chrome
/// through the slot's `plan`, which picks the `unchecked` or `checked`
/// four-state pack, check glyph color from `indicator`, geometry from
/// `box_size` etc.
#[derive(Debug)]
pub struct Checkbox<'a> {
    widget: Widget,
    value: &'a mut bool,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a> Checkbox<'a> {
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        Self {
            widget: ToggleChrome::row(),
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    /// The text this widget draws. Empty (the default) draws none —
    /// no text child is recorded at all.
    ///
    /// Drawn to the right of the box; an empty label leaves the box alone.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `checkbox`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a ToggleTheme>>) -> Self {
        self.style = s.into();
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let response = self.widget.response(ui);

        let checked = ToggleChrome::toggled(&response, self.value);

        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.checkbox);
        let box_size = slot.box_size.themed_length(1.0);
        let indicator = slot.indicator;
        let indicator_stroke = slot.indicator_stroke.themed_length(0.0);
        let check = slot.check_polyline();
        let chrome = ToggleChrome {
            plan: slot.plan(&response, checked, theme.text),
            gap: slot.gap,
            boxed: Widget::leaf().size((Sizing::fixed(box_size), Sizing::fixed(box_size))),
            // Square box: the theme's own corner radius stands.
            pill: None,
        };
        chrome.record_row(ui, self.widget, response, self.label, |ui, _| {
            if checked {
                ui.add_shape(
                    Shape::polyline(&check, PolylineColors::Single(indicator), indicator_stroke)
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            }
        })
    }
}

impl Configure for Checkbox<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
