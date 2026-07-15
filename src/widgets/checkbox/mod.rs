use crate::forest::element::{Configure, Element};
use crate::input::sense::Sense;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::interned_str::InternedStr;
use crate::shape::{LineCap, LineJoin, PolylineColors, Shape};
use crate::ui::Ui;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::toggle::{ToggleChrome, toggle_row};
use crate::widgets::{Response, WidgetEntry, enter_widget};
use glam::Vec2;

/// Two-state boolean toggle. Takes a `&mut bool` whose owner controls
/// the value — same pattern as egui. Clicking the row flips it.
///
/// Layout: HStack [box, label]. The whole row is one hit target with
/// `Sense::CLICK`; clicking anywhere on it toggles. Child node ids
/// derive from the outer widget id via `WidgetId::with`, so they stay
/// stable across sibling insertions (no reliance on `SeenIds`'
/// occurrence-counter disambiguation).
///
/// Visuals come from `theme.checkbox` ([`crate::ToggleTheme`]) —
/// chrome via `unchecked.pick(state)` / `checked.pick(state)`, check
/// glyph color from `indicator`, geometry from `box_size` etc.
#[derive(Debug)]
pub struct Checkbox<'a> {
    element: Element,
    value: &'a mut bool,
    label: InternedStr,
    style: Option<ToggleTheme>,
}

impl<'a> Checkbox<'a> {
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.flags.set_sense(Sense::CLICK);
        Self {
            element,
            value,
            label: InternedStr::default(),
            style: None,
        }
    }

    pub fn label(mut self, s: impl Into<InternedStr>) -> Self {
        self.label = s.into();
        self
    }

    /// Override the theme for this checkbox. `None` (default) inherits
    /// [`crate::Theme::checkbox`].
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
        if state.left.clicked() && !state.disabled {
            *self.value = !*self.value;
        }
        let checked = *self.value;

        let theme = self.style.as_ref().unwrap_or(&ui.theme.checkbox);
        let chrome = ToggleChrome::new(theme, state, checked, false);
        let indicator = theme.indicator;
        let indicator_stroke = theme.indicator_stroke;

        toggle_row(
            ui,
            id,
            self.element,
            raw_state,
            chrome,
            self.label,
            |ui, box_size| {
                if checked {
                    let pts = check_pts(box_size);
                    ui.add_shape(
                        Shape::polyline(&pts, PolylineColors::Single(indicator), indicator_stroke)
                            .cap(LineCap::Round)
                            .join(LineJoin::Round),
                    );
                }
            },
        )
    }
}

impl Configure for Checkbox<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

// Three-point checkmark normalized to the box square. Coords were
// hand-tuned at 16 px and scale linearly with `box_size`.
fn check_pts(box_size: f32) -> [Vec2; 3] {
    let s = box_size / 16.0;
    [
        Vec2::new(3.5 * s, 8.5 * s),
        Vec2::new(7.0 * s, 12.0 * s),
        Vec2::new(12.5 * s, 4.5 * s),
    ]
}

#[cfg(test)]
mod tests;
