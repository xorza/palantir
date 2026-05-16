use crate::forest::element::{Configure, Element, LayoutMode};
use crate::layout::types::align::Align;
use crate::primitives::interned_str::InternedStr;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::text_style::TextStyle;

/// Standalone shaped-text leaf. Use for labels, paragraphs, headings —
/// anything that's just a string. Hugs its measured size by default; call
/// `.wrapping()` to opt into reshape-on-arrange when a fixed-width parent
/// commits a narrower width than the natural unbroken line.
///
/// Style is all-or-nothing: the optional `style` field replaces every
/// text axis (font size, color, leading) at once. Defaults to the
/// global [`crate::TextStyle`] from [`crate::Theme::text`] when not set.
/// To tweak one axis, build a `TextStyle` from the theme and override
/// the field you want:
///
/// ```ignore
/// Text::new("hi").auto_id().style(TextStyle { color: red, ..ui.theme.text })
/// ```
pub struct Text {
    element: Element,
    text: InternedStr<'static>,
    style: Option<TextStyle>,
    wrap: TextWrap,
    align: Align,
}

impl Text {
    #[track_caller]
    pub fn new(text: impl Into<InternedStr<'static>>) -> Self {
        Self {
            element: Element::new(LayoutMode::Leaf),
            text: text.into(),
            style: None,
            wrap: TextWrap::Single,
            // Default = (Auto, Auto) → top-left. Only matters when the
            // widget has Fixed size larger than its measured content;
            // a Hug Text widget has no slack to align in.
            align: Align::default(),
        }
    }

    /// Override the whole text style for this run. All-or-nothing —
    /// every axis the bundle covers (font size, color, leading) is
    /// replaced. To tweak one axis, build the bundle from the theme:
    /// `TextStyle { color: red, ..ui.theme.text }`.
    pub fn style(mut self, s: TextStyle) -> Self {
        self.style = Some(s);
        self
    }

    /// Allow the renderer to reshape this text at the arranged width when
    /// the parent commits a narrower width than the unbounded line. Without
    /// this, the text just hugs its widest natural line forever.
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
        let style = self.style.unwrap_or(ui.theme.text);
        let line_height_px = style.line_height_for(style.font_size_px);
        ui.node(self.element, |ui| {
            ui.add_shape(Shape::Text {
                local_origin: None,
                text: self.text,
                brush: style.color.into(),
                font_size_px: style.font_size_px,
                line_height_px,
                wrap: self.wrap,
                align: self.align,
                family: style.family,
            });
        });
        let state = ui.response_for(id);
        Response { id, state }
    }
}

impl Configure for Text {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
