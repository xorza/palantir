use crate::layout::types::{align::Align, sense::Sense};
use crate::primitives::spacing::Spacing;
use crate::shape::{Shape, TextWrap};
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::ui::Ui;
use crate::widgets::theme::ButtonTheme;
use crate::widgets::{Response, frame::Frame};
use std::borrow::Cow;

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

        // Frame paints the per-state background. `None` inherits
        // `Background::default()` (transparent, no stroke, zero radius);
        // `Ui::add_shape` filters that as a no-op shape.
        let resp = Frame::for_element(element)
            .background(v.background.unwrap_or_default())
            .show(ui);

        // Layer the label on top of the frame's background. Safe immediately after
        // `Frame::show` because no other shape/node has been pushed since the
        // frame closed — `Tree::add_shape`'s contiguity invariant still holds.
        if !self.label.is_empty() {
            // Per-state text style; `None` falls through to the global
            // `Theme::text`, so an app changing `theme.text.color`
            // moves every button label that didn't override it.
            let text = v.text.unwrap_or_else(|| ui.theme.text.clone());
            ui.tree.add_shape(
                resp.node,
                Shape::Text {
                    text: self.label.clone(),
                    color: text.color,
                    font_size_px: text.font_size_px,
                    line_height_px: text.font_size_px * text.line_height_mult,
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
