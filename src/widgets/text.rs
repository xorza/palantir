use crate::element::{Configure, Element, LayoutMode};
use crate::primitives::{Color, WidgetId};
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use glam::Vec2;
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
    /// this, the text just hugs its widest natural line forever.
    pub fn wrapping(mut self) -> Self {
        self.wrap = TextWrap::Wrap { intrinsic_min: 0.0 };
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let max_w = match self.wrap {
            TextWrap::Single => None,
            // Shape unbounded for measurement; the reshape-during-arrange
            // pass narrows it later if the parent commits a tighter width.
            TextWrap::Wrap { .. } => None,
        };
        let m = ui.measure_text(&self.text, self.size_px, max_w);
        let wrap = match self.wrap {
            TextWrap::Single => TextWrap::Single,
            TextWrap::Wrap { .. } => TextWrap::Wrap {
                intrinsic_min: m.intrinsic_min,
            },
        };
        let node = ui.node(self.element, |ui| {
            ui.add_shape(Shape::Text {
                offset: Vec2::ZERO,
                text: self.text.into_owned(),
                color: self.color,
                measured: m.size,
                font_size_px: self.size_px,
                max_width_px: max_w,
                key: m.key,
                wrap,
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
