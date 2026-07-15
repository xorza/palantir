use crate::forest::element::{Configure, Element};
use crate::layout::types::align::Align;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::interned_str::InternedStr;
use crate::shape::{Shape, TextWrap};
use crate::text::FontWeight;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::text_style::TextStyle;

/// Standalone shaped-text leaf. Use for labels, paragraphs, headings —
/// anything that's just a string. Hugs its measured size when it has room;
/// **by default a single-line label keeps its full natural width**
/// ([`TextWrap::SingleLine`]) — its min-content equals its full line, so a
/// Hug parent / grid track sizes to it and never shrinks it below its text
/// (the label "stays natural"); if a parent commits a width narrower than
/// the line, the line runs past the slot rather than being silently cut.
/// Use [`Self::text_wrap`] to opt into clipping or wrapping instead:
/// `Truncate` hard-cuts to the committed width (no marker), `Ellipsis`
/// marks the cut with `…`, `Wrap` / `WrapWithOverflow` reflow onto multiple
/// lines. Widgets that should clip a too-long label (e.g. `Button`,
/// `DragValue`) set `SingleLine` explicitly.
///
/// Style is all-or-nothing: the optional `style` field replaces every
/// text axis (font size, color, leading) at once. Defaults to the
/// global [`crate::TextStyle`] from [`crate::Theme::text`] when not set.
/// To tweak one axis, build a `TextStyle` from the theme and override
/// the field you want:
///
/// ```ignore
/// Text::new("hi").style(TextStyle { color: red, ..ui.theme.text })
/// ```
#[derive(Debug)]
pub struct Text {
    element: Element,
    text: InternedStr,
    style: Option<TextStyle>,
    /// Single-axis weight override applied over the resolved `style` in
    /// `show`. Lets `Text::new("x").bold()` request bold without cloning
    /// the whole ambient `TextStyle` at the call site.
    weight: Option<FontWeight>,
    wrap: TextWrap,
    align: Align,
}

impl Text {
    #[track_caller]
    pub fn new(text: impl Into<InternedStr>) -> Self {
        Self {
            element: Element::new(LayoutMode::Leaf),
            text: text.into(),
            style: None,
            weight: None,
            wrap: TextWrap::SingleLine,
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

    /// Shape this run bold, overriding just the weight of the resolved
    /// style (whether that came from `.style(...)` or the theme default).
    pub fn bold(mut self) -> Self {
        self.weight = Some(FontWeight::Bold);
        self
    }

    /// Set how the text handles a committed width narrower than its natural
    /// line. Default [`TextWrap::SingleLine`] (one unbroken line that runs past
    /// the slot; its min-content is the full line width, so a Hug track won't
    /// shrink below it — the label keeps its natural width). Pass
    /// [`TextWrap::Truncate`] to hard-cut to the committed width with no
    /// marker, [`TextWrap::Ellipsis`] to mark the cut with `…`, or
    /// [`TextWrap::WrapWithOverflow`] to reshape onto multiple lines.
    pub fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.wrap = wrap;
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

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut style = self.style.unwrap_or(ui.theme.text);
        if let Some(weight) = self.weight {
            style.weight = weight;
        }
        let line_height_px = style.line_height_for(style.font_size_px);
        let id = ui.widget_id(&self.element);
        ui.node(id, self.element, None, |ui| {
            ui.add_shape(Shape::Text {
                local_origin: None,
                text: self.text,
                brush: style.color.into(),
                font_size_px: style.font_size_px,
                line_height_px,
                wrap: self.wrap,
                align: self.align,
                family: style.family,
                weight: style.weight,
            });
        });
        // Decorative: skip eager `response_for`. Discarded responses
        // pay zero; a `.left.clicked()` call later does one lazy probe.
        Response::lazy(id, ui)
    }
}

impl Configure for Text {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
