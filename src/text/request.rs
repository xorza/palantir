//! What every shaping call is asked in terms of: source text paired with
//! the canonical parameters it shapes under.

use crate::common::hash;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::{TextShapeKey, WrapBound};

/// Source text paired with its canonical shaping parameters.
///
/// **The crate's one empty-text boundary.** A run with no bytes shapes
/// nothing and mints no buffer, so there is no request to make of the
/// shaper: both constructors answer `None` for it, and the fields are
/// private so nothing can assemble one around them. Every layer past this
/// type therefore holds text it can shape, which is why none of them —
/// the reuse slots, the dispatch, either measurer — carries a guard of
/// its own.
///
/// Only two things still meet an empty run, and both are crate edges with
/// an answer of their own rather than a layer to hop through:
/// [`TextShaper::layout`](crate::text::shaper::TextShaper::layout) mints
/// an empty probe, and [`TextGlyphs`](crate::TextGlyphs) reports no
/// glyphs. Recorded runs never reach either — `TextShape::is_noop` drops
/// an empty one before it becomes a `ShapeRecord`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextShapeRequest<'a> {
    pub(super) text: &'a str,
    pub(super) key: TextShapeKey,
}

impl<'a> TextShapeRequest<'a> {
    /// Metrics were validated at record time; invalid values here are a
    /// logic error, debug-asserted by [`TextShapeKey::unbounded`].
    ///
    /// Hashes `text` itself. A caller holding the hash already — layout
    /// retains one per recorded run — mints the key and pairs it through
    /// [`Self::for_key`] rather than paying for it twice.
    ///
    /// `None` for empty text — see the type docs.
    pub(crate) fn unbounded(text: &'a str, font: GlyphFont) -> Option<Self> {
        (!text.is_empty()).then(|| Self {
            text,
            key: TextShapeKey::unbounded(hash::hash_str(text), font),
        })
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
    ///
    /// `None` for empty text — see the type docs.
    pub(crate) fn for_key(text: &'a str, key: TextShapeKey) -> Option<Self> {
        debug_assert_eq!(
            key.text_hash,
            TextShapeKey::content_hash(hash::hash_str(text)),
            "text paired with a key minted from different bytes",
        );
        (!text.is_empty()).then_some(Self { text, key })
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

// Wider than `cfg(test)`, unlike its sibling in `text::root`: the text
// benches state one const face as a `TestShape` and lower it through
// `unbounded_request`. Everything else here is assertion-side and says so
// item by item.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    use super::*;
    #[cfg(test)]
    use crate::layout::types::align::HAlign;
    #[cfg(test)]
    use crate::text::wrap::LineFit;
    // The face is a `GlyphFont` now, so only the assertion-side builders
    // that override one of its enums still name them.
    #[cfg(test)]
    use crate::text::{FontFamily, FontWeight};

    /// A shaping request's parameters without its text, so a test can
    /// describe one face once and measure many strings through it.
    /// Override with struct-update syntax.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TestShape {
        pub(crate) font: GlyphFont,
        /// Assertion-side, with [`TestShape::halign`]: the benches build
        /// one const face and shape it unbounded, so only a test ever
        /// binds a width or asks for an alignment.
        #[cfg(test)]
        pub(crate) max_width_px: Option<f32>,
        #[cfg(test)]
        pub(crate) halign: HAlign,
    }

    /// Field reads a case asserts on. Production never needs them: every
    /// layer that holds a request either shapes it or forwards it whole,
    /// and the two paths that ask about a committed width read the key
    /// they already have.
    #[cfg(test)]
    impl<'a> TextShapeRequest<'a> {
        /// The bytes this request shapes. Never empty — see the type docs.
        pub(crate) fn text(self) -> &'a str {
            self.text
        }

        /// The shaped-buffer key this request is cached under.
        pub(crate) fn key(self) -> TextShapeKey {
            self.key
        }

        /// The width this request commits to, or `None` when it is the
        /// run's unbounded root — the question that picks between the
        /// shaper's two measure paths.
        pub(crate) fn max_width_px(self) -> Option<f32> {
            self.key.max_width_px()
        }
    }

    impl TestShape {
        /// Fixtures always name text to shape, so the empty-run boundary
        /// is a wiring bug here rather than a case a test drives — the
        /// two crate edges that answer one do it in their own tests.
        pub(crate) fn unbounded_request<'a>(self, text: &'a str) -> TextShapeRequest<'a> {
            TextShapeRequest::unbounded(text, self.font)
                .expect("a shaping fixture needs text to shape")
        }
    }

    /// Builders, because a case almost always wants one or two overrides
    /// off a named base. Struct-update syntax spells that across four
    /// lines and buries the override among the fields it inherits; a
    /// chain puts what the case is *about* on one line.
    ///
    /// Named for the field each sets, so `shape(16.0).halign(Right)` and
    /// a `halign:` literal stay obviously the same thing.
    ///
    /// Assertion-side as a block: a bench states one face as a const and
    /// lowers it unbounded, so overriding a field and binding a width are
    /// both things only a test does.
    #[cfg(test)]
    impl TestShape {
        pub(crate) fn font_size(self, size_px: f32) -> Self {
            Self {
                font: GlyphFont {
                    size_px,
                    ..self.font
                },
                ..self
            }
        }

        pub(crate) fn leading(self, line_height_px: f32) -> Self {
            Self {
                font: GlyphFont {
                    line_height_px,
                    ..self.font
                },
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

        /// Bound to this shape's width under `fit`, or unbounded where it
        /// has no width.
        ///
        /// Takes the [`LineFit`] rather than a [`TextWrap`](crate::TextWrap)
        /// because this is a shaper-level fixture: `LineFit` is what
        /// [`TextShapeKey`] stores and what `CosmicMeasure::resolve`
        /// switches on, while the policy is layout's to resolve. So the
        /// gate here is a width alone, where `TextRun::request` gates on
        /// `(width, wrap.line_fit())`.
        ///
        /// The two still reach the same requests, which is what keeps a
        /// fixture from describing a run layout cannot produce: both bind
        /// through [`WrapBound::new`], a shape with no width lowers to
        /// exactly what a non-binding policy does, and every [`LineFit`]
        /// is some policy's — pinned by `wrap`'s
        /// `every_line_fit_is_some_policys_…` rather than restated here,
        /// since a copy of that mapping is a copy that can go stale.
        pub(crate) fn request<'a>(self, text: &'a str, fit: LineFit) -> TextShapeRequest<'a> {
            let request = self.unbounded_request(text);
            match self.max_width_px {
                Some(width) => request.with_bound(WrapBound::new(width, self.halign, fit)),
                None => request,
            }
        }
    }
}
