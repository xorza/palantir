//! What every shaping call is asked in terms of: source text paired with
//! the canonical parameters it shapes under.

use crate::common::hash;
use crate::text::key::{TextShapeKey, WrapBound};
use crate::text::{FontFamily, FontWeight};

/// Source text paired with its canonical shaping parameters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextShapeRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) key: TextShapeKey,
}

impl<'a> TextShapeRequest<'a> {
    /// Metrics were validated at record time; invalid values here are a
    /// logic error, debug-asserted by [`TextShapeKey::unbounded`].
    ///
    /// Hashes `text` itself. A caller holding the hash already — layout
    /// retains one per recorded run — mints the key and pairs it through
    /// [`Self::for_key`] rather than paying for it twice.
    pub(crate) fn unbounded(
        text: &'a str,
        font_size_px: f32,
        line_height_px: f32,
        family: FontFamily,
        weight: FontWeight,
    ) -> Self {
        Self {
            text,
            key: TextShapeKey::unbounded(
                hash::hash_str(text),
                font_size_px,
                line_height_px,
                family,
                weight,
            ),
        }
    }

    /// Pair `text` with a key already minted for it — off a hash layout
    /// retained, or off a key carried through the paint payload.
    ///
    /// **The one place a key is checked against the bytes themselves.**
    /// That a key describes the text beside it is what makes reusing a
    /// cached shaped buffer sound, and every caller that built the pair by
    /// hand used to carry its own version of this assertion — so one could
    /// drift, or be forgotten by the next caller to write the literal.
    /// (`ShapedTextRef::new` checks the same pairing against a *recorded*
    /// hash, which costs no re-read; this is the one that reads.)
    pub(crate) fn for_key(text: &'a str, key: TextShapeKey) -> Self {
        debug_assert_eq!(
            key.text_hash,
            TextShapeKey::content_hash(hash::hash_str(text)),
            "text paired with a key minted from different bytes",
        );
        Self { text, key }
    }

    pub(super) fn with_bound(self, bound: WrapBound) -> Self {
        Self {
            key: self.key.with_bound(bound),
            ..self
        }
    }

    pub(super) fn unbounded_version(self) -> Self {
        Self {
            key: self.key.unbounded_version(),
            ..self
        }
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    // See [`crate::text::root::internals`] — same gate, same reason.
    #![allow(dead_code)]
    use super::*;
    use crate::layout::types::align::HAlign;
    use crate::text::wrap::LineFit;

    /// A shaping request's parameters without its text, so a test can
    /// describe one face once and measure many strings through it.
    /// Override with struct-update syntax.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TestShape {
        pub(crate) font_size_px: f32,
        pub(crate) line_height_px: f32,
        pub(crate) max_width_px: Option<f32>,
        pub(crate) family: FontFamily,
        pub(crate) weight: FontWeight,
        pub(crate) halign: HAlign,
    }

    /// Builders, because a case almost always wants one or two overrides
    /// off a named base. Struct-update syntax spells that across four
    /// lines and buries the override among the fields it inherits; a
    /// chain puts what the case is *about* on one line.
    ///
    /// Named for the field each sets, so `shape(16.0).halign(Right)` and
    /// a `halign:` literal stay obviously the same thing.
    impl TestShape {
        pub(crate) fn font_size(self, font_size_px: f32) -> Self {
            Self {
                font_size_px,
                ..self
            }
        }

        pub(crate) fn leading(self, line_height_px: f32) -> Self {
            Self {
                line_height_px,
                ..self
            }
        }

        pub(crate) fn width(self, max_width_px: f32) -> Self {
            Self {
                max_width_px: Some(max_width_px),
                ..self
            }
        }

        /// The unbounded root of this shape: no wrap width, and no
        /// per-line alignment, which only means anything with one.
        pub(crate) fn unbounded(self) -> Self {
            Self {
                max_width_px: None,
                halign: HAlign::Auto,
                ..self
            }
        }

        pub(crate) fn halign(self, halign: HAlign) -> Self {
            Self { halign, ..self }
        }

        pub(crate) fn family(self, family: FontFamily) -> Self {
            Self { family, ..self }
        }

        pub(crate) fn weight(self, weight: FontWeight) -> Self {
            Self { weight, ..self }
        }

        pub(crate) fn unbounded_request<'a>(self, text: &'a str) -> TextShapeRequest<'a> {
            TextShapeRequest::unbounded(
                text,
                self.font_size_px,
                self.line_height_px,
                self.family,
                self.weight,
            )
        }

        pub(crate) fn request<'a>(self, text: &'a str, fit: LineFit) -> TextShapeRequest<'a> {
            let request = self.unbounded_request(text);
            match self.max_width_px {
                Some(width) => request.with_bound(WrapBound::new(width, self.halign, fit)),
                None => request,
            }
        }
    }
}
