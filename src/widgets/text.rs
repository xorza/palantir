//! The standalone text leaf: labels, paragraphs and headings.

use crate::primitives::layout::align::Align;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::theme::text_style::{TextStyle, TextStyleOverrides};

/// Standalone shaped-text leaf for labels, paragraphs and headings. **By default a
/// single-line label keeps its full natural width** ([`TextWrap::SingleLine`]): its
/// min-content is the full line, so a Hug parent never shrinks it and a narrower
/// committed width lets the line run past the slot. [`Self::text_wrap`] opts into
/// `Truncate`, `Ellipsis`, or wrapping; widgets that should clip (`Button`,
/// `DragValue`) set `Truncate`.
///
/// # Styling
///
/// [`Self::style`] replaces every text axis at once (default:
/// [`crate::Theme::text`]); each axis also has its own setter ([`Self::color`],
/// [`Self::font_size`], [`Self::family`], [`Self::weight`], [`Self::slant`],
/// [`Self::line_height_factor`]) overriding just that axis of the resolved bundle:
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
/// The font size is [`Self::font_size`], not `size`, which [`Configure::size`] uses
/// for layout extent.
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
    /// A run of `text`: a `&str`, a `String`, or the
    /// [`InternedStr`](crate::InternedStr) that [`fmt!`](crate::fmt) mints.
    #[track_caller]
    pub fn new(text: impl Into<TextInput<'a>>) -> Self {
        Self {
            widget: Widget::leaf(),
            text: text.into(),
            style: None,
            overrides: TextStyleOverrides::NONE,
            wrap: TextWrap::SingleLine,
            align: Align::default(),
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `text`. All-or-nothing, so
    /// `.style(&heading).color(red)` is that bundle in red.
    pub fn style(mut self, s: impl Into<Option<&'a TextStyle>>) -> Self {
        self.style = s.into();
        self
    }

    /// Fill colour for this run, overriding the resolved style's.
    ///
    /// # Panics
    ///
    /// Panics unless `color` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, color: RgbaF32) -> Self {
        self.overrides.color = Some(domain::color(color));
        self
    }

    /// Font size in logical px, a *length*, overriding the resolved style's; named
    /// apart from [`Configure::size`].
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn font_size(mut self, px: f32) -> Self {
        self.overrides.font_size = Some(domain::length(px));
        self
    }

    /// Line height as a multiple of the font size, overriding the resolved style's;
    /// `1.0` sets lines solid. `factor`: *positive*.
    ///
    /// # Panics
    ///
    /// Panics unless `factor` is [positive](crate::widget::domain::positive).
    #[track_caller]
    pub const fn line_height_factor(mut self, factor: f32) -> Self {
        self.overrides.line_height_factor = Some(domain::positive(factor));
        self
    }

    /// Family to shape against, overriding the resolved style's.
    pub const fn family(mut self, family: FontFamily) -> Self {
        self.overrides.family = Some(family);
        self
    }

    /// Weight to shape against; [`Self::bold`] is `FontWeight::BOLD`.
    pub const fn weight(mut self, weight: FontWeight) -> Self {
        self.overrides.weight = Some(weight);
        self
    }

    /// Upright or italic; [`Self::italic`] is `FontSlant::Italic`.
    pub const fn slant(mut self, slant: FontSlant) -> Self {
        self.overrides.slant = Some(slant);
        self
    }

    /// Shape this run bold.
    pub const fn bold(mut self) -> Self {
        self.overrides.weight = Some(FontWeight::BOLD);
        self
    }

    /// Shape this run italic; the weight is untouched, so `.bold().italic()` is
    /// bold italic.
    pub const fn italic(mut self) -> Self {
        self.overrides.slant = Some(FontSlant::Italic);
        self
    }

    /// How the text handles a committed width narrower than its natural line;
    /// default [`TextWrap::SingleLine`].
    pub const fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.wrap = wrap;
        self
    }

    /// Position of the glyph bbox inside this widget's arranged rect, distinct from
    /// [`Configure::align`], which positions the *widget*. Meaningful only when the
    /// widget is Fixed larger than the text.
    pub const fn text_align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    /// Record the run; it senses nothing until [`Configure::sense`] says otherwise.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        // Folded back into a `TextStyle` so the `line_height_factor` formula has
        // one owner.
        let style = self.overrides.apply(self.style.unwrap_or(&ui.theme().text));
        let color = style.color;
        let font = style.font();
        // No metrics guard: `TextShape::is_noop` rejects a non-finite size or
        // leading at `add_shape`.
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
