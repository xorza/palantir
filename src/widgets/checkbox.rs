use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
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
/// `Sense::CLICK`; clicking anywhere on it toggles.
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
        let mut this = Self {
            element,
            value,
            label: InternedStr::default(),
        };
        this = this.gap(ROW_GAP).child_align(Align::v(VAlign::Center));
        this
    }

    pub fn label(mut self, s: impl Into<InternedStr<'static>>) -> Self {
        self.label = s.into();
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let state = ui.response_for(id);
        if state.clicked {
            *self.value = !*self.value;
        }
        let checked = *self.value;
        let disabled = state.disabled;
        let hovered = state.hovered && !disabled;
        let pressed = state.pressed && !disabled;

        let BoxVisuals {
            chrome: box_chrome,
            check_color,
        } = box_visuals(ui, checked, hovered, pressed, disabled);
        let label_color = label_color(ui, disabled);
        let text_style = ui.theme.text;
        let label = self.label;

        ui.node(self.element, |ui| {
            // The box.
            let mut box_elem = Element::new(LayoutMode::Leaf);
            box_elem.size = (Sizing::Fixed(BOX_SIZE), Sizing::Fixed(BOX_SIZE)).into();
            ui.node_with_chrome(box_elem, box_chrome, |ui| {
                if checked {
                    let pts = check_polyline_pts();
                    ui.add_shape(Shape::Polyline {
                        points: &pts,
                        colors: PolylineColors::Single(check_color),
                        width: CHECK_STROKE,
                        cap: LineCap::Round,
                        join: LineJoin::Round,
                    });
                }
            });

            if !label.is_empty() {
                let mut label_elem = Element::new(LayoutMode::Leaf);
                label_elem.size = (Sizing::Hug, Sizing::Hug).into();
                ui.node(label_elem, |ui| {
                    ui.add_shape(Shape::Text {
                        local_origin: None,
                        text: label,
                        brush: label_color.into(),
                        font_size_px: text_style.font_size_px,
                        line_height_px: text_style.line_height_for(text_style.font_size_px),
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

fn check_polyline_pts() -> [Vec2; 3] {
    // Three-point checkmark inside a `BOX_SIZE` square (16px), in
    // node-local coords: down-stroke to the elbow, then up-stroke
    // to the top-right.
    [
        Vec2::new(3.5, 8.5),
        Vec2::new(7.0, 12.0),
        Vec2::new(12.5, 4.5),
    ]
}

struct BoxVisuals {
    chrome: Background,
    /// Color to stroke the checkmark with (only used when `checked`).
    check_color: Color,
}

fn box_visuals(ui: &Ui, checked: bool, hovered: bool, pressed: bool, disabled: bool) -> BoxVisuals {
    // Derive colors from the button theme so a Checkbox visually fits
    // next to a Button without a dedicated theme. Checked fills with
    // text color (foreground accent on dark themes); unchecked uses
    // the button's normal/hover/pressed/disabled fills.
    let btn = &ui.theme.button;
    let state = if disabled {
        &btn.disabled
    } else if pressed {
        &btn.pressed
    } else if hovered {
        &btn.hovered
    } else {
        &btn.normal
    };
    let base = state.background.unwrap_or_default();
    let radius = Corners::all(BOX_RADIUS);
    let text_color = state.text.map(|t| t.color).unwrap_or(ui.theme.text.color);

    if checked {
        // Fill with the text color; check stroke uses the panel bg
        // so it reads as a "punch-out". Falls back to the window
        // clear if no panel bg is set.
        let fill = if disabled {
            with_alpha(text_color, 0.55)
        } else {
            text_color
        };
        let punch = ui
            .theme
            .panel_background
            .and_then(|bg| match bg.fill {
                Brush::Solid(c) => Some(c),
                _ => None,
            })
            .unwrap_or(ui.theme.window_clear);
        BoxVisuals {
            chrome: Background {
                fill: fill.into(),
                stroke: base.stroke,
                radius,
                shadow: Shadow::NONE,
            },
            check_color: punch,
        }
    } else {
        BoxVisuals {
            chrome: Background {
                fill: base.fill,
                stroke: if base.stroke.is_noop() {
                    Stroke::solid(with_alpha(text_color, 0.35), 1.0)
                } else {
                    base.stroke
                },
                radius,
                shadow: Shadow::NONE,
            },
            check_color: text_color,
        }
    }
}

fn label_color(ui: &Ui, disabled: bool) -> Color {
    if disabled {
        ui.theme
            .button
            .disabled
            .text
            .map(|t| t.color)
            .unwrap_or(ui.theme.text.color)
    } else {
        ui.theme.text.color
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color::linear_rgba(c.r, c.g, c.b, a)
}
