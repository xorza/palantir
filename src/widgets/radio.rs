use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::toggle::toggle_row;

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
        }
    }

    pub fn label(mut self, s: impl Into<InternedStr>) -> Self {
        self.label = s.into();
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let id = ui.make_persistent_id(self.element.salt);
        let raw_state = ui.response_for(id);
        let mut state = raw_state;
        state.disabled |= self.element.flags.is_disabled();
        let selected = *self.current == self.value;
        // Radios latch — re-clicking the selected option is a no-op,
        // matches platform behavior on every OS.
        if state.clicked && !state.disabled && !selected {
            *self.current = self.value;
        }

        let theme = &ui.theme.radio;
        let look_target = theme.pick(state, selected).clone();
        let row_gap = theme.row_gap;
        let pip_size = theme.box_size;
        let dot_inset = theme.indicator_inset;
        let anim = theme.anim;
        let indicator = theme.indicator;
        let fallback_text = ui.theme.text;

        // Force pill radius regardless of any look's stored radius so a
        // re-themed `theme.radio.checked.normal.background.radius`
        // can't accidentally square-corner the pip. Baked into the look
        // before `toggle_row` records the box chrome.
        let mut look = look_target.animate(ui, id, fallback_text, anim);
        look.background.corners = Corners::all(pip_size * 0.5);

        toggle_row(
            ui,
            id,
            self.element,
            raw_state,
            look,
            pip_size,
            row_gap,
            self.label,
            |ui| {
                if selected {
                    let dot_size = pip_size - 2.0 * dot_inset;
                    let dot = Rect::new(dot_inset, dot_inset, dot_size, dot_size);
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(dot),
                        corners: Corners::all(dot_size * 0.5),
                        fill: indicator.into(),
                        stroke: Stroke::ZERO,
                    });
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
