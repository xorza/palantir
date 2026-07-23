use crate::input::sense::Sense;
use crate::primitives::interned_str::TextInput;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::toggle::{ToggleChrome, toggle_row};
use crate::widgets::{Response, enter_widget};
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
    node: Node,
    value: &'a mut bool,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a> Checkbox<'a> {
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Borrow a theme override for this checkbox. The default inherits
    /// [`crate::Theme::checkbox`].
    pub fn style(mut self, s: &'a ToggleTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let entry = enter_widget(ui, self.node);
        let state = &entry.state;
        if state.left.clicked() && !state.disabled {
            *self.value = !*self.value;
        }
        let checked = *self.value;

        let theme = self.style.unwrap_or(&ui.theme.checkbox);
        let chrome = ToggleChrome::new(theme, state, checked, false);
        let indicator = theme.indicator;
        let indicator_stroke = theme.indicator_stroke;

        toggle_row(ui, entry, chrome, self.label, |ui, box_size| {
            if checked {
                let pts = check_pts(box_size);
                ui.add_shape(
                    Shape::polyline(&pts, PolylineColors::Single(indicator), indicator_stroke)
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            }
        })
    }
}

impl Configure for Checkbox<'_> {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
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
