use crate::layout::types::{align::Align, sense::Sense};
use crate::primitives::{color::Color, corners::Corners};
use crate::shape::{Shape, TextWrap};
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::ui::Ui;
use crate::widgets::theme::{Background, TextStyle};
use crate::widgets::{Response, frame::Frame};
use std::borrow::Cow;

/// Paint settings for one button state — `normal`, `hovered`,
/// `pressed`, or `disabled`. Each `Option` field follows the same rule:
/// `Some(x)` overrides; `None` inherits the framework default for that
/// field. `background = None` inherits [`Background::default`] (a
/// transparent / no-stroke / zero-radius background, which paints
/// nothing — `Ui::add_shape` filters no-op shapes). `text = None`
/// inherits [`crate::Theme::text`] (the global text style), so an app
/// changing `theme.text.color` moves every button label that didn't
/// override it.
///
/// Used as the leaf type of [`ButtonTheme`]'s four state slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonStateStyle {
    pub background: Option<Background>,
    pub text: Option<TextStyle>,
}

/// Four-state button theme. The leaf type ([`ButtonStateStyle`]) lives next
/// to it; widget reads `theme.{normal,hovered,pressed,disabled}` based
/// on the live response state and `Element::disabled`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonTheme {
    pub normal: ButtonStateStyle,
    pub hovered: ButtonStateStyle,
    pub pressed: ButtonStateStyle,
    pub disabled: ButtonStateStyle,
}

impl Default for ButtonTheme {
    fn default() -> Self {
        // Each state's Background carries the historical 4 px radius.
        // `text: None` on normal/hovered/pressed means "use the global
        // text style" — bumping `theme.text.color` automatically
        // recolors active button labels. Disabled has its own faded
        // text since the global default is opaque.
        let bg = |fill: Color| -> Option<Background> {
            Some(Background {
                fill,
                stroke: None,
                radius: Corners::all(4.0),
            })
        };
        Self {
            normal: ButtonStateStyle {
                background: bg(Color::rgb(0.20, 0.40, 0.80)),
                text: None,
            },
            hovered: ButtonStateStyle {
                background: bg(Color::rgb(0.30, 0.52, 0.92)),
                text: None,
            },
            pressed: ButtonStateStyle {
                background: bg(Color::rgb(0.10, 0.28, 0.66)),
                text: None,
            },
            disabled: ButtonStateStyle {
                background: bg(Color::rgb(0.22, 0.26, 0.32)),
                text: Some(TextStyle::default().with_color(Color::rgba(1.0, 1.0, 1.0, 0.45))),
            },
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

        // Frame paints the per-state background. `None` inherits
        // `Background::default()` (transparent, no stroke, zero radius);
        // `Ui::add_shape` filters that as a no-op shape.
        let resp = Frame::for_element(self.element)
            .background(v.background.unwrap_or_default())
            .show(ui);

        // Layer the label on top of the frame's background. Safe immediately after
        // `Frame::show` because no other shape/node has been pushed since the
        // frame closed — `Tree::add_shape`'s contiguity invariant still holds.
        if !self.label.is_empty() {
            // Per-state text style; `None` falls through to the global
            // `Theme::text`, so an app changing `theme.text.color`
            // moves every button label that didn't override it.
            let text = v.text.unwrap_or(ui.theme.text);
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
