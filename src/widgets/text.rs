use crate::layout::types::align::Align;
use crate::primitives::interned_str::TextInput;
use crate::scene::element::{Configure, ConfigureElement, Element};
use crate::shape::Shape;
use crate::text::FontWeight;
use crate::text::wrap::TextWrap;
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
/// let style = TextStyle { color: red, ..ui.theme.text.clone() };
/// Text::new("hi").style(&style)
/// ```
#[derive(Debug)]
pub struct Text<'a> {
    element: Element,
    text: TextInput<'a>,
    style: Option<&'a TextStyle>,
    /// Single-axis weight override applied over the resolved `style` in
    /// `show`. Lets `Text::new("x").bold()` request bold without cloning
    /// the whole ambient `TextStyle` at the call site.
    weight: Option<FontWeight>,
    wrap: TextWrap,
    align: Align,
}

impl<'a> Text<'a> {
    #[track_caller]
    pub fn new(text: impl Into<TextInput<'a>>) -> Self {
        Self {
            element: Element::leaf(),
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
    /// `TextStyle { color: red, ..ui.theme.text.clone() }`.
    pub fn style(mut self, s: &'a TextStyle) -> Self {
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
        let style = self.style.unwrap_or(&ui.theme.text);
        let color = style.color;
        let family = style.family;
        let weight = self.weight.unwrap_or(style.weight);
        let metrics_valid = style.metrics().is_some();
        let font_size_px = style.font_size_px;
        let line_height_px = style.line_height_for(font_size_px);
        let widget = ui.widget(self.element);
        widget.node(ui, None, |ui| {
            if metrics_valid {
                let text = ui.intern(self.text);
                ui.add_shape(Shape::Text {
                    local_origin: None,
                    text,
                    color,
                    font_size_px,
                    line_height_px,
                    wrap: self.wrap,
                    align: self.align,
                    family,
                    weight,
                });
            }
        });
        // Decorative: skip eager `response_for`. Discarded responses
        // pay zero; a `.left.clicked()` call later does one lazy probe.
        widget.response(ui)
    }
}

impl Configure for Text<'_> {
    fn element_mut(&mut self) -> ConfigureElement<'_> {
        self.element.element_mut()
    }
}
