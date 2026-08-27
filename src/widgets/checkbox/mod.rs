use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::text_input::TextInput;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::theme::widget_look::theme_slot::ThemeSlot;
use crate::widgets::toggle_chrome::ToggleChrome;

/// Two-response boolean toggle. Takes a `&mut bool` whose owner controls
/// the value — same pattern as egui. Clicking the row flips it.
///
/// Layout: HStack [box, label]. The whole row is one hit target with
/// `Sense::CLICK`; clicking anywhere on it toggles. Child node ids
/// derive from the outer widget id via `WidgetId::with`, so they stay
/// stable across sibling insertions (no reliance on `SeenIds`'
/// occurrence-counter disambiguation).
///
/// Visuals come from `theme.checkbox` ([`crate::ToggleTheme`]) —
/// chrome via `unchecked.pick(response)` / `checked.pick(response)`, check
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

    label_setter!('a, "Drawn to the right of the box; an empty label leaves the box alone.");

    style_setter!('a, ToggleTheme, checkbox);

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let widget = ui.widget(self.node);
        let response = widget.response(ui);

        if response.left.clicked() && !response.disabled {
            *self.value = !*self.value;
        }
        let checked = *self.value;

        let theme = ui.theme();
        let slot = self.slot(theme);
        let box_size = slot.box_size;
        let indicator = slot.indicator;
        let indicator_stroke = slot.indicator_stroke;
        let check = slot.check_polyline();
        let chrome = ToggleChrome {
            plan: slot.plan(&response, checked, &theme.text),
            row_gap: slot.row_gap,
            boxed: Node::leaf().size((Sizing::fixed(box_size), Sizing::fixed(box_size))),
            // Square box: the theme's own corner radius stands.
            pill: None,
        };
        chrome.record_row(ui, widget, response, self.label, |ui, _| {
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

impl_configure!(Checkbox<'_>);

#[cfg(test)]
mod tests;
