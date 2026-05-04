use crate::element::{Configure, Element, LayoutMode};
use crate::primitives::{align::Align, color::Color, widget_id::WidgetId};
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use std::borrow::Cow;
use std::hash::Hash;

/// Default font size when no `.size(px)` was set. 16 px lines up with
/// `Button`'s historical default and with the `mono_measure` fallback's
/// reference metric.
const DEFAULT_SIZE_PX: f32 = 16.0;

/// Standalone shaped-text leaf. Use for labels, paragraphs, headings —
/// anything that's just a string. Hugs its measured size by default; call
/// `.wrapping()` to opt into reshape-on-arrange when a fixed-width parent
/// commits a narrower width than the natural unbroken line.
pub struct Text {
    element: Element,
    text: Cow<'static, str>,
    size_px: f32,
    color: Color,
    wrap: TextWrap,
    align: Align,
}

impl Text {
    #[track_caller]
    pub fn new(text: impl Into<Cow<'static, str>>) -> Self {
        Self::with_id_inner(WidgetId::auto_stable(), text.into())
    }

    pub fn with_id(id: impl Hash, text: impl Into<Cow<'static, str>>) -> Self {
        Self::with_id_inner(WidgetId::from_hash(id), text.into())
    }

    fn with_id_inner(id: WidgetId, text: Cow<'static, str>) -> Self {
        Self {
            element: Element::new(id, LayoutMode::Leaf),
            text,
            size_px: DEFAULT_SIZE_PX,
            color: Color::WHITE,
            wrap: TextWrap::Single,
            // Default = (Auto, Auto) → top-left. Only matters when the
            // widget has Fixed size larger than its measured content;
            // a Hug Text widget has no slack to align in.
            align: Align::default(),
        }
    }

    pub fn size_px(mut self, px: f32) -> Self {
        assert!(px > 0.0, "Text size must be positive, got {px}");
        self.size_px = px;
        self
    }

    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }

    /// Allow the renderer to reshape this text at the arranged width when
    /// the parent commits a narrower width than the unbounded line. Without
    /// Reshape this text at the arranged width when the parent commits a
    /// narrower width than the unbounded line. Without this, the text just
    /// hugs its widest natural line forever.
    pub fn wrapping(mut self) -> Self {
        self.wrap = TextWrap::Wrap;
        self
    }

    /// Position of the glyph bbox inside this text widget's arranged
    /// rect. Distinct from [`Configure::align`], which positions the
    /// *widget* inside its parent's slot. Only meaningful when the
    /// widget has Fixed size larger than the text's measured size;
    /// otherwise the widget hugs its content and there's no slack to
    /// align in.
    pub fn text_align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let node = ui.node(self.element, |ui| {
            ui.add_shape(Shape::Text {
                text: self.text.into_owned(),
                color: self.color,
                font_size_px: self.size_px,
                wrap: self.wrap,
                align: self.align,
            });
        });
        let state = ui.response_for(id);
        Response { node, state }
    }
}

impl Configure for Text {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
