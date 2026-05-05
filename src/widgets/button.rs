use crate::layout::types::{align::Align, sense::Sense};
use crate::primitives::{color::Color, corners::Corners};
use crate::shape::{Shape, TextWrap};
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::ui::Ui;
use crate::widgets::theme::{Background, TextStyle};
use crate::widgets::{Response, frame::Frame};
use std::borrow::Cow;

/// Paint settings for one button state — `normal`, `hovered`,
/// `pressed`, or `disabled`. Each carries its own `Background` (fill /
/// stroke / radius) and `TextStyle` (font size / color / leading), so
/// two states with different radii or font sizes is expressible — most
/// themes keep them equal across states by convention.
///
/// Used as the leaf type of [`ButtonTheme`]'s four state slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonStyle {
    pub background: Option<Background>,
    // todo also optional
    pub text: TextStyle,
}

impl ButtonStyle {
    /// Compatibility constructor: solid fill + text color, default
    /// font/leading from `TextStyle::default()`. Background carries no
    /// stroke and zero radius — set those by spreading from this.
    pub fn solid(fill: Color, text_color: Color) -> Self {
        Self {
            background: Some(Background {
                fill,
                ..Background::default()
            }),
            text: TextStyle::default().with_color(text_color),
        }
    }
}

/// Four-state button theme. The leaf type ([`ButtonStyle`]) lives next
/// to it; widget reads `theme.{normal,hovered,pressed,disabled}` based
/// on the live response state and `Element::disabled`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonTheme {
    pub normal: ButtonStyle,
    pub hovered: ButtonStyle,
    pub pressed: ButtonStyle,
    pub disabled: ButtonStyle,
}

impl Default for ButtonTheme {
    fn default() -> Self {
        // Per-state ButtonStyle share a Background with the historical
        // 4 px radius. `solid` doesn't set the radius, so we adjust
        // each state below.
        let with_radius = |v: ButtonStyle| -> ButtonStyle {
            ButtonStyle {
                background: v.background.map(|b| Background {
                    radius: Corners::all(4.0),
                    ..b
                }),
                ..v
            }
        };
        Self {
            normal: with_radius(ButtonStyle::solid(
                Color::rgb(0.20, 0.40, 0.80),
                Color::WHITE,
            )),
            hovered: with_radius(ButtonStyle::solid(
                Color::rgb(0.30, 0.52, 0.92),
                Color::WHITE,
            )),
            pressed: with_radius(ButtonStyle::solid(
                Color::rgb(0.10, 0.28, 0.66),
                Color::WHITE,
            )),
            disabled: with_radius(ButtonStyle::solid(
                Color::rgb(0.22, 0.26, 0.32),
                Color::rgba(1.0, 1.0, 1.0, 0.45),
            )),
        }
    }
}

pub struct Button {
    element: Element,
    style: Option<ButtonTheme>,
    label: Cow<'static, str>,
    label_align: Align,
}

impl Button {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut element = Element::new_auto(LayoutMode::Leaf);
        element.sense = Sense::CLICK;
        Self {
            element,
            style: None,
            label: Cow::Borrowed(""),
            // Buttons center their labels by convention. Override with
            // `.text_align(...)` for left/right-aligned labels.
            label_align: Align::CENTER,
        }
    }

    pub fn style(mut self, s: ButtonTheme) -> Self {
        self.style = Some(s);
        self
    }
    pub fn label(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        self.label = s.into();
        self
    }

    /// Position of the label glyphs inside the button's arranged rect.
    /// Distinct from [`Configure::align`], which positions the *button*
    /// inside its parent's slot. Default: [`Align::CENTER`].
    pub fn text_align(mut self, a: Align) -> Self {
        self.label_align = a;
        self
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let style = self.style.unwrap_or(ui.theme.button);
        let v = if self.element.disabled {
            style.disabled
        } else {
            let state = ui.response_for(self.element.id);
            if state.pressed {
                style.pressed
            } else if state.hovered {
                style.hovered
            } else {
                style.normal
            }
        };

        // Frame paints the per-state background. `None` skips it.
        let mut frame = Frame::for_element(self.element);
        if let Some(bg) = v.background {
            frame = frame.background(bg);
        }
        let resp = frame.show(ui);

        // Layer the label on top of the frame's background. Safe immediately after
        // `Frame::show` because no other shape/node has been pushed since the
        // frame closed — `Tree::add_shape`'s contiguity invariant still holds.
        if !self.label.is_empty() {
            ui.tree.add_shape(
                resp.node,
                Shape::Text {
                    text: self.label.clone(),
                    color: v.text.color,
                    font_size_px: v.text.font_size_px,
                    line_height_px: v.text.font_size_px * v.text.line_height_mult,
                    wrap: TextWrap::Single,
                    align: self.label_align,
                },
            );
        }

        resp
    }
}

impl Configure for Button {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
