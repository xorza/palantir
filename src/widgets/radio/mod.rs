use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::interned_str::TextInput;
use crate::primitives::rect::Rect;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::theme::WidgetTheme;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::toggle::{self, ToggleChrome};

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
#[derive(Debug)]
pub struct RadioButton<'a, T: PartialEq> {
    node: Node,
    current: &'a mut T,
    value: T,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a, T: PartialEq> RadioButton<'a, T> {
    #[track_caller]
    pub fn new(current: &'a mut T, value: T) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            current,
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    style_setter!('a, ToggleTheme, radio);

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut widget = ui.widget(self.node);
        let response = widget.response(ui);
        let id = widget.id();

        // Everything this widget takes off its theme slot, before
        // `toggle_row`'s `&mut Ui` reborrow: the geometry it paints with, and
        // the plan for the look. `toggle_row` is shared by three toggles
        // reading three different slots, so which slot is `RadioButton`'s own
        // business — and `style_setter!`'s `slot` is where it says so, once.
        //
        // Ahead of the latch below, which moves `self.value` and so leaves
        // `self` unborrowable.
        let theme = ui.theme();
        let slot = self.slot(theme);
        let pip_size = slot.box_size;
        let indicator = slot.indicator;
        let dot_inset = slot.indicator_inset;
        let row_gap = slot.row_gap;

        let mut selected = *self.current == self.value;
        // Radios latch — re-clicking the selected option is a no-op,
        // matches platform behavior on every OS. A fresh click selects
        // this option, so flip `selected` now (`value` is moved into
        // `current`, so we can't re-derive it) — otherwise the chrome +
        // pip below paint unselected until the next unrelated repaint.
        if response.left.clicked() && !response.disabled && !selected {
            *self.current = self.value;
            selected = true;
        }

        let look = slot
            .plan(&theme.text, &response, selected)
            .apply(ui, id, &mut widget.node);

        let chrome = ToggleChrome {
            look,
            row_gap,
            boxed: Node::leaf().size((Sizing::fixed(pip_size), Sizing::fixed(pip_size))),
            // Forces the pip chrome to a circle regardless of any
            // re-themed `radio.checked.normal.background.radius` — a
            // radio pip must never square-corner.
            pill: Some(pip_size * 0.5),
        };
        toggle::toggle_row(ui, widget, response, chrome, self.label, |ui, _| {
            if selected {
                let dot_size = pip_size - 2.0 * dot_inset;
                let dot = Rect::new(dot_inset, dot_inset, dot_size, dot_size);
                ui.add_shape(Shape::rect(dot).corners(dot_size * 0.5).fill(indicator));
            }
        })
    }
}

impl<T: PartialEq> Configure for RadioButton<'_, T> {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

#[cfg(test)]
mod tests;
