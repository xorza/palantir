use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::interned_str::InternedStr;
use crate::shape::{LineCap, LineJoin, PolylineColors, Shape, TextWrap};
use crate::ui::UiCore;
use crate::widgets::Response;
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
pub struct Checkbox<'a> {
    element: Element,
    value: &'a mut bool,
    label: InternedStr<'static>,
}

impl<'a> Checkbox<'a> {
    #[track_caller]
    pub fn new(value: &'a mut bool) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.set_sense(Sense::CLICK);
        Self {
            element,
            value,
            label: InternedStr::default(),
        }
    }

    pub fn label(mut self, s: impl Into<InternedStr<'static>>) -> Self {
        self.label = s.into();
        self
    }

    pub fn show(self, ui: &mut UiCore) -> Response {
        let id = self.element.id;
        let raw_state = ui.response_for(id);
        let mut state = raw_state;
        // Cascade lags by a frame; OR self-disabled in so a freshly
        // disabled checkbox doesn't toggle or paint hovered on its
        // first frame. Mirrors Button.
        state.disabled |= self.element.is_disabled();
        if state.clicked && !state.disabled {
            *self.value = !*self.value;
        }
        let checked = *self.value;

        let theme = &ui.theme.checkbox;
        let look_target = *theme.pick(state, checked);
        let row_gap = theme.row_gap;
        let box_size = theme.box_size;
        let indicator_stroke = theme.indicator_stroke;
        let anim = theme.anim;
        let indicator = theme.indicator;
        let fallback_text = ui.theme.text;

        let look = look_target.animate(ui, id, fallback_text, anim);
        let chrome = look.background;
        let label = self.label;

        let mut element = self.element;
        element.set_gap(row_gap);
        element.set_child_align(Align::v(VAlign::Center));

        ui.node(element, |ui| {
            let mut box_elem = Element::new(LayoutMode::Leaf);
            box_elem.set_id(id.with("box"));
            box_elem.size = (Sizing::Fixed(box_size), Sizing::Fixed(box_size)).into();
            ui.node_with_chrome(box_elem, chrome, |ui| {
                if checked {
                    let pts = check_pts(box_size);
                    ui.add_shape(Shape::Polyline {
                        points: &pts,
                        colors: PolylineColors::Single(indicator),
                        width: indicator_stroke,
                        cap: LineCap::Round,
                        join: LineJoin::Round,
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
