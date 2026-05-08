use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::primitives::spacing::Spacing;
use crate::shape::{Shape, TextWrap};
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::{ButtonTheme, Surface};
use std::borrow::Cow;

pub struct Button {
    element: Element,
    style: Option<ButtonTheme>,
    label: Cow<'static, str>,
    label_align: Align,
}

impl Button {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
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
        let style = self
            .style
            .clone()
            .unwrap_or_else(|| ui.theme.button.clone());
        // Apply theme padding/margin when the builder hasn't set
        // anything (sentinel: `Spacing::ZERO` == "use theme"). User
        // overrides — anything non-zero set via `.padding(...)` /
        // `.margin(...)` — pass through unchanged.
        let mut element = self.element;
        if element.padding == Spacing::ZERO {
            element.padding = style.padding;
        }
        if element.margin == Spacing::ZERO {
            element.margin = style.margin;
        }
        let v = if element.disabled {
            style.disabled
        } else {
            let state = ui.response_for(element.id);
            if state.pressed {
                style.pressed
            } else if state.hovered {
                style.hovered
            } else {
                style.normal
            }
        };

        let surface = Some(Surface::from(v.background.unwrap_or_default()));
        let text_style = v.text.unwrap_or_else(|| ui.theme.text.clone());
        let label = self.label.clone();
        let label_align = self.label_align;

        let node = ui.node(element, surface, |ui| {
            if !label.is_empty() {
                ui.add_shape(Shape::Text {
                    local_rect: None,
                    text: label,
                    color: text_style.color,
                    font_size_px: text_style.font_size_px,
                    line_height_px: text_style.font_size_px * text_style.line_height_mult,
                    wrap: TextWrap::Single,
                    align: label_align,
                });
            }
        });
        let state = ui.response_for(element.id);
        Response { node, state }
    }
}

impl Configure for Button {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
