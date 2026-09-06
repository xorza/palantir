//! The standalone text leaf: labels, paragraphs and headings, shaped and
//! measured like any other content.

use crate::layout::types::align::Align;
use crate::primitives::color::RgbaF32;
use crate::primitives::text_input::TextInput;
use crate::shape::Shape;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::response::Response;
use crate::widgets::theme::text_style::{TextStyle, TextStyleOverrides};
use crate::widgets::widget::Widget;

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
/// `DragValue`) set `Truncate` explicitly.
///
/// # Styling
///
/// [`Self::style`] replaces every text axis at once, and defaults to the
/// global [`crate::TextStyle`] from [`crate::Theme::text`]. Each axis also
/// has a setter of its own — [`Self::color`], [`Self::font_size`],
/// [`Self::family`], [`Self::weight`], [`Self::slant`],
/// [`Self::line_height`] — which overrides that one axis of whatever the
/// bundle resolved to:
///
/// ```
/// # use palantir::{FontWeight, RgbaF32, Text, Ui};
/// # fn demo(ui: &mut Ui) {
/// Text::new("hi")
///     .color(RgbaF32::hex(0xd94f4f))
///     .font_size(20.0)
///     .weight(FontWeight::BOLD)
///     .show(ui);
/// # }
/// ```
///
/// The font size is [`Self::font_size`] and not `size`, because
/// [`Configure::size`](crate::Configure::size) already names the widget's
/// layout extent.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Text<'a> {
    widget: Widget,
    text: TextInput<'a>,
    style: Option<&'a TextStyle>,
    overrides: TextStyleOverrides,
    wrap: TextWrap,
    align: Align,
}

impl<'a> Text<'a> {
    /// A run of `text`, which takes a `&str`, a `String`, or the
    /// [`InternedStr`](crate::InternedStr) that [`fmt!`](crate::fmt) mints.
    #[track_caller]
    pub fn new(text: impl Into<TextInput<'a>>) -> Self {
        Self {
            widget: Widget::leaf(),
            text: text.into(),
            style: None,
            overrides: TextStyleOverrides::default(),
            wrap: TextWrap::SingleLine,
            // Default = (Auto, Auto) → top-left. Only matters when the
            // widget has Fixed size larger than its measured content;
            // a Hug Text widget has no slack to align in.
            align: Align::default(),
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `text`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    ///
    /// All-or-nothing — every axis the bundle covers is replaced. The
    /// per-axis setters below then override single axes of the result, so
    /// `.style(&heading).color(red)` is that bundle in red.
    pub fn style(mut self, s: impl Into<Option<&'a TextStyle>>) -> Self {
        self.style = s.into();
        self
    }

    /// Fill colour for this run, overriding the resolved style's.
    pub fn color(mut self, color: RgbaF32) -> Self {
        self.overrides.color = Some(color);
        self
    }

    /// Font size in logical px, overriding the resolved style's.
    ///
    /// Named apart from [`Configure::size`](crate::Configure::size), which
    /// is the widget's layout extent.
    pub fn font_size(mut self, px: f32) -> Self {
        self.overrides.font_size_px = Some(px);
        self
    }

    /// Line height as a multiple of the font size, overriding the resolved
    /// style's `line_height_mult`. `1.0` sets the lines solid.
    pub fn line_height(mut self, mult: f32) -> Self {
        self.overrides.line_height_mult = Some(mult);
        self
    }

    /// Family to shape against, overriding the resolved style's.
    pub fn family(mut self, family: FontFamily) -> Self {
        self.overrides.family = Some(family);
        self
    }

    /// Weight to shape against, overriding the resolved style's.
    /// [`Self::bold`] is this with [`FontWeight::BOLD`].
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.overrides.weight = Some(weight);
        self
    }

    /// Upright or italic, overriding the resolved style's.
    /// [`Self::italic`] is this with [`FontSlant::Italic`].
    pub fn slant(mut self, slant: FontSlant) -> Self {
        self.overrides.slant = Some(slant);
        self
    }

    /// Shape this run bold — [`Self::weight`] with [`FontWeight::BOLD`].
    pub fn bold(mut self) -> Self {
        self.overrides.weight = Some(FontWeight::BOLD);
        self
    }

    /// Shape this run italic — [`Self::slant`] with
    /// [`FontSlant::Italic`]. The weight axis is untouched, so
    /// `.bold().italic()` is bold italic.
    pub fn italic(mut self) -> Self {
        self.overrides.slant = Some(FontSlant::Italic);
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
    /// rect. Distinct from [`Configure::align`](crate::Configure::align), which positions the
    /// *widget* inside its parent's slot. Only meaningful when the
    /// widget has Fixed size larger than the text's measured size;
    /// otherwise the widget hugs its content and there's no slack to
    /// align in.
    pub fn text_align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    /// Record the run. It senses nothing until [`Configure::sense`] says
    /// otherwise.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        // Folded back into a `TextStyle` rather than a `GlyphFont`, so the
        // `line_height_mult` formula keeps its one owner.
        let style = self.overrides.apply(self.style.unwrap_or(&ui.theme().text));
        let color = style.color;
        let font = style.font();
        // No metrics guard here: `TextShape::is_noop` rejects a non-finite
        // size or leading at `add_shape`, which is where Button and
        // DragValue leave it too. One owner of the rule, and it is the one
        // downstream of every recorder.
        self.widget
            .show(ui, None, |ui| {
                let text = ui.intern(self.text);
                ui.add_shape(
                    Shape::text(text, font)
                        .color(color)
                        .wrap(self.wrap)
                        .align(self.align),
                );
            })
            .response
    }
}

impl Configure for Text<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}
