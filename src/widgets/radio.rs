use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::ResponseState;
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
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
/// `Sense::CLICK` hit target spanning the whole row.
pub struct RadioButton<'a, T: PartialEq> {
    element: Element,
    current: &'a mut T,
    value: T,
    label: InternedStr<'static>,
}

const PIP_SIZE: f32 = 16.0;
const DOT_INSET: f32 = 4.0;
const ROW_GAP: f32 = 8.0;

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
        .gap(ROW_GAP)
        .child_align(Align::v(VAlign::Center))
    }

    pub fn label(mut self, s: impl Into<InternedStr<'static>>) -> Self {
        self.label = s.into();
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let mut state = ui.response_for(id);
        state.disabled |= self.element.is_disabled();
        let selected = *self.current == self.value;
        // Radios latch — re-clicking the selected option is a no-op,
        // matches platform behavior on every OS.
        if state.clicked && !state.disabled && !selected {
            *self.current = self.value;
        }

        let RadioVisuals {
            pip_chrome,
            dot_color,
            label_color,
        } = visuals(ui, state, selected);
        let text_style = ui.theme.text;
        let label = self.label;
        let line_height_px = text_style.line_height_for(text_style.font_size_px);

        ui.node(self.element, |ui| {
            let mut pip_elem = Element::new(LayoutMode::Leaf);
            pip_elem.set_id(id.with("pip"));
            pip_elem.size = (Sizing::Fixed(PIP_SIZE), Sizing::Fixed(PIP_SIZE)).into();
            ui.node_with_chrome(pip_elem, pip_chrome, |ui| {
                if let Some(c) = dot_color {
                    let dot_size = PIP_SIZE - 2.0 * DOT_INSET;
                    let dot = Rect::new(DOT_INSET, DOT_INSET, dot_size, dot_size);
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(dot),
                        radius: Corners::all(dot_size * 0.5),
                        fill: c.into(),
                        stroke: Stroke::ZERO,
                    });
                }
            });

            if !label.is_empty() {
                let mut label_elem = Element::new(LayoutMode::Leaf);
                label_elem.set_id(id.with("label"));
                ui.node(label_elem, |ui| {
                    ui.add_shape(Shape::Text {
                        local_origin: None,
                        text: label,
                        brush: label_color.into(),
                        font_size_px: text_style.font_size_px,
                        line_height_px,
                        wrap: TextWrap::Single,
                        align: Align::v(VAlign::Center),
                        family: text_style.family,
                    });
                });
            }
        });

        Response { id, state }
    }
}

impl<T: PartialEq> Configure for RadioButton<'_, T> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

struct RadioVisuals {
    pip_chrome: Background,
    /// `Some` only when the pip should paint the inner dot.
    dot_color: Option<Color>,
    label_color: Color,
}

fn visuals(ui: &Ui, state: ResponseState, selected: bool) -> RadioVisuals {
    let base = ui.theme.button.pick(state).background.unwrap_or_default();
    let fg = if state.disabled {
        ui.theme.text.color.with_alpha(0.45)
    } else {
        ui.theme.text.color
    };
    RadioVisuals {
        pip_chrome: Background {
            fill: base.fill,
            stroke: if base.stroke.is_noop() {
                Stroke::solid(ui.theme.text.color.with_alpha(0.35), 1.0)
            } else {
                base.stroke
            },
            radius: Corners::all(PIP_SIZE * 0.5),
            shadow: Shadow::NONE,
        },
        dot_color: selected.then_some(fg),
        label_color: fg,
    }
}
