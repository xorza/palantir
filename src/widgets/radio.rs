use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;

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
    label: InternedStr<'static>,
}

impl<'a, T: PartialEq> RadioButton<'a, T> {
    #[track_caller]
    pub fn new(current: &'a mut T, value: T) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.set_sense(Sense::CLICK);
        Self {
            element,
            current,
            value,
            label: InternedStr::default(),
        }
    }

    pub fn label(mut self, s: impl Into<InternedStr<'static>>) -> Self {
        self.label = s.into();
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let id = ui.make_persistent_id(self.element.salt);
        let raw_state = ui.response_for(id);
        let mut state = raw_state;
        state.disabled |= self.element.is_disabled();
        let selected = *self.current == self.value;
        // Radios latch — re-clicking the selected option is a no-op,
        // matches platform behavior on every OS.
        if state.clicked && !state.disabled && !selected {
            *self.current = self.value;
        }

        let theme = &ui.theme.radio;
        let look_target = *theme.pick(state, selected);
        let row_gap = theme.row_gap;
        let pip_size = theme.box_size;
        let dot_inset = theme.indicator_inset;
        let anim = theme.anim;
        let indicator = theme.indicator;
        let fallback_text = ui.theme.text;

        // Force pill radius regardless of any look's stored radius so a
        // re-themed `theme.radio.checked.normal.background.radius`
        // can't accidentally square-corner the pip.
        let mut look = look_target.animate(ui, id, fallback_text, anim);
        look.background.radius = Corners::all(pip_size * 0.5);
        let chrome = look.background;
        let label = self.label;

        let mut element = self.element;
        element.set_gap(row_gap);
        element.set_child_align(Align::v(VAlign::Center));

        ui.node(id, element, |ui| {
            let pip_id = id.with("pip");
            let mut pip_elem = Element::new(LayoutMode::Leaf);
            pip_elem.salt = Salt::Verbatim(pip_id);
            pip_elem.size = (Sizing::Fixed(pip_size), Sizing::Fixed(pip_size)).into();
            ui.node_with_chrome(pip_id, pip_elem, chrome, |ui| {
                if selected {
                    let dot_size = pip_size - 2.0 * dot_inset;
                    let dot = Rect::new(dot_inset, dot_inset, dot_size, dot_size);
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(dot),
                        radius: Corners::all(dot_size * 0.5),
                        fill: indicator.into(),
                        stroke: Stroke::ZERO,
                    });
                }
            });

            if !label.is_empty() {
                let label_id = id.with("label");
                let mut label_elem = Element::new(LayoutMode::Leaf);
                label_elem.salt = Salt::Verbatim(label_id);
                ui.node(label_id, label_elem, |ui| {
                    ui.add_shape(Shape::Text {
                        local_origin: None,
                        text: label,
                        brush: look.text.color.into(),
                        font_size_px: look.text.font_size_px,
                        line_height_px: look.line_height_px(),
                        wrap: TextWrap::Single,
                        align: Align::v(VAlign::Center),
                        family: look.text.family,
                    });
                });
            }
        });

        Response {
            id,
            state: raw_state,
        }
    }
}

impl<T: PartialEq> Configure for RadioButton<'_, T> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
