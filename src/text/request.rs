//! Source text paired with the canonical parameters it shapes under.

use crate::common::hash;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::{TextShapeKey, WrapBound};

/// Source text paired with its canonical shaping parameters.
///
/// **The crate's one nothing-to-shape boundary.** A run with no bytes, or whose
/// face names no usable size, shapes nothing: both constructors answer `None` and
/// the fields are private, so every layer past this type holds a shapeable run and
/// needs no guard.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextShapeRequest<'a> {
    pub(super) text: &'a str,
    pub(super) key: TextShapeKey,
}

impl<'a> TextShapeRequest<'a> {
    /// **Where a face is screened.** `font` can arrive straight from a public
    /// [`TextRun`](crate::widget::TextRun) or
    /// [`TextGlyphs`](crate::widget::TextGlyphs) call, so the screen sits at the
    /// boundary both cross. Hashes `text` itself; a caller holding the hash uses
    /// [`Self::for_key`]. `None` for empty text or an unusable face.
    pub(crate) fn unbounded(text: &'a str, font: GlyphFont) -> Option<Self> {
        let key = TextShapeKey::for_text(text, font)?;
        (!text.is_empty()).then_some(Self { text, key })
    }

    /// Pair `text` with a key already minted for it. **The one place a key is
    /// checked against the bytes**, so cached buffer reuse is sound; debug-only, as
    /// re-hashing costs `O(n)` per run per frame. `None` for empty text.
    pub(crate) fn for_key(text: &'a str, key: TextShapeKey) -> Option<Self> {
        debug_assert_eq!(
            key.text_hash,
            TextShapeKey::content_hash(hash::hash_str(text)),
            "text paired with a key minted from different bytes",
        );
        (!text.is_empty()).then_some(Self { text, key })
    }

    pub(super) const fn with_bound(self, bound: WrapBound) -> Self {
        Self {
            key: self.key.with_bound(bound),
            ..self
        }
    }

    pub(super) const fn unbounded_version(self) -> Self {
        Self {
            key: self.key.unbounded_version(),
            ..self
        }
    }
}

// Wider than `cfg(test)`: the text benches lower one const face through
// `unbounded_request`.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use super::*;
    #[cfg(test)]
    use crate::primitives::layout::align::HAlign;
    #[cfg(test)]
    use crate::text::font_family::FontFamily;
    #[cfg(test)]
    use crate::text::font_slant::FontSlant;
    #[cfg(test)]
    use crate::text::font_weight::FontWeight;
    #[cfg(test)]
    use crate::text::wrap::LineFit;

    /// A shaping request's parameters without its text, so a test describes one
    /// face once and measures many strings.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TestShape {
        pub(crate) font: GlyphFont,
        /// Assertion-side: benches shape unbounded, so only tests bind a width or
        /// alignment.
        #[cfg(test)]
        pub(crate) max_width: Option<f32>,
        #[cfg(test)]
        pub(crate) halign: HAlign,
    }

    /// Field reads for assertions; production forwards a request whole.
    #[cfg(test)]
    impl<'a> TextShapeRequest<'a> {
        pub(crate) fn text(self) -> &'a str {
            self.text
        }

        pub(crate) fn key(self) -> TextShapeKey {
            self.key
        }

        pub(crate) fn max_width(self) -> Option<f32> {
            self.key.max_width()
        }
    }

    impl TestShape {
        pub(crate) const fn new(font: GlyphFont) -> Self {
            Self {
                font,
                #[cfg(test)]
                max_width: None,
                #[cfg(test)]
                halign: HAlign::Auto,
            }
        }

        /// Fixtures always name text and a usable face, so the nothing-to-shape
        /// boundary is a wiring bug here.
        pub(crate) fn unbounded_request(self, text: &str) -> TextShapeRequest<'_> {
            TextShapeRequest::unbounded(text, self.font)
                .expect("a shaping fixture needs text and a usable face")
        }
    }

    /// Builders for the one or two overrides a case wants; named for the field each
    /// sets. Assertion-side: only a test overrides a field or binds a width.
    #[cfg(test)]
    impl TestShape {
        pub(crate) fn font_size(self, size: f32) -> Self {
            Self {
                font: GlyphFont { size, ..self.font },
                ..self
            }
        }

        pub(crate) fn leading(self, line_height: f32) -> Self {
            Self {
                font: GlyphFont {
                    line_height,
                    ..self.font
                },
                ..self
            }
        }

        pub(crate) fn width(self, max_width: f32) -> Self {
            Self {
                max_width: Some(max_width),
                ..self
            }
        }

        pub(crate) fn unbounded(self) -> Self {
            Self {
                max_width: None,
                halign: HAlign::Auto,
                ..self
            }
        }

        pub(crate) fn halign(self, halign: HAlign) -> Self {
            Self { halign, ..self }
        }

        pub(crate) fn family(self, family: FontFamily) -> Self {
            Self {
                font: GlyphFont {
                    family,
                    ..self.font
                },
                ..self
            }
        }

        pub(crate) fn weight(self, weight: FontWeight) -> Self {
            Self {
                font: GlyphFont {
                    weight,
                    ..self.font
                },
                ..self
            }
        }

        pub(crate) fn slant(self, slant: FontSlant) -> Self {
            Self {
                font: GlyphFont { slant, ..self.font },
                ..self
            }
        }

        /// Bound to this shape's width under `fit`, or unbounded where it has none.
        /// Takes the [`LineFit`] rather than a [`TextWrap`](crate::TextWrap): this
        /// is a shaper-level fixture, so the gate is a width alone where
        /// `TextShaper::layout` gates on `(width, wrap.line_fit())`. Both bind
        /// through [`WrapBound::new`].
        pub(crate) fn request(self, text: &str, fit: LineFit) -> TextShapeRequest<'_> {
            let request = self.unbounded_request(text);
            match self.max_width {
                Some(width) => request.with_bound(WrapBound::new(width, self.halign, fit)),
                None => request,
            }
        }
    }
}
