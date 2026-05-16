use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::ResponseState;
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::shape::{LineCap, LineJoin, PolylineColors, Shape, TextWrap};
use crate::ui::Ui;
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
pub struct Checkbox<'a> {
    element: Element,
    value: &'a mut bool,
    label: InternedStr<'static>,
}

const BOX_SIZE: f32 = 16.0;
const BOX_RADIUS: f32 = 3.0;
const ROW_GAP: f32 = 8.0;
const CHECK_STROKE: f32 = 2.0;

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
        // Cascade lags by a frame; OR self-disabled in so a freshly
        // disabled checkbox doesn't toggle or paint hovered on its
        // first frame. Mirrors Button.
        state.disabled |= self.element.is_disabled();
        if state.clicked && !state.disabled {
            *self.value = !*self.value;
        }
        let checked = *self.value;

        let CheckboxVisuals {
            box_chrome,
            check_color,
            label_color,
        } = visuals(ui, state, checked);
        let text_style = ui.theme.text;
        let label = self.label;
        let line_height_px = text_style.line_height_for(text_style.font_size_px);

        ui.node(self.element, |ui| {
            let mut box_elem = Element::new(LayoutMode::Leaf);
            box_elem.set_id(id.with("box"));
            box_elem.size = (Sizing::Fixed(BOX_SIZE), Sizing::Fixed(BOX_SIZE)).into();
            ui.node_with_chrome(box_elem, box_chrome, |ui| {
                if let Some(c) = check_color {
                    let pts = CHECK_PTS;
                    ui.add_shape(Shape::Polyline {
                        points: &pts,
                        colors: PolylineColors::Single(c),
                        width: CHECK_STROKE,
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

impl Configure for Checkbox<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

// Three-point checkmark in box-local coords. Hand-tuned for the
// `BOX_SIZE = 16.0` square; the const-assert pins the coupling.
const _: () = assert!(BOX_SIZE == 16.0);
const CHECK_PTS: [Vec2; 3] = [
    Vec2::new(3.5, 8.5),
    Vec2::new(7.0, 12.0),
    Vec2::new(12.5, 4.5),
];

struct CheckboxVisuals {
    box_chrome: Background,
    /// `Some` only when the box should paint a checkmark.
    check_color: Option<Color>,
    label_color: Color,
}

fn visuals(ui: &Ui, state: ResponseState, checked: bool) -> CheckboxVisuals {
    // Reach into `theme.button` for state-driven fills so a checkbox
    // visually matches buttons side-by-side without a dedicated theme.
    // The button theme's per-state *text* overrides aren't appropriate
    // for an unrelated widget; foreground comes from `theme.text`.
    // Promote to a `CheckboxTheme` when the framework grows one.
    let base = ui.theme.button.pick(state).background.unwrap_or_default();
    let radius = Corners::all(BOX_RADIUS);
    let fg = if state.disabled {
        ui.theme.text.color.with_alpha(0.45)
    } else {
        ui.theme.text.color
    };

    if checked {
        // Filled box paints with the foreground; the checkmark uses
        // `window_clear` so it reads as "punched out" against the row.
        CheckboxVisuals {
            box_chrome: Background {
                fill: fg.into(),
                stroke: Stroke::ZERO,
                radius,
                shadow: Shadow::NONE,
            },
            check_color: Some(ui.theme.window_clear),
            label_color: fg,
        }
    } else {
        CheckboxVisuals {
            box_chrome: Background {
                fill: base.fill,
                stroke: if base.stroke.is_noop() {
                    Stroke::solid(ui.theme.text.color.with_alpha(0.35), 1.0)
                } else {
                    base.stroke
                },
                radius,
                shadow: Shadow::NONE,
            },
            check_color: None,
            label_color: fg,
        }
    }
}
