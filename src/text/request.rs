//! What every shaping call is asked in terms of: source text paired with
//! the canonical parameters it shapes under.

use crate::common::hash;
use crate::layout::types::align::HAlign;
use crate::text::key::TextShapeKey;
use crate::text::wrap::LineFit;
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

    pub(crate) fn bounded(self, max_width_px: f32, halign: HAlign, fit: LineFit) -> Self {
        Self {
            text: self.text,
            key: self.key.bounded(max_width_px, halign, fit),
        }
    }

    pub(crate) fn unbounded_version(self) -> Self {
        Self {
            text: self.text,
            key: self.key.unbounded_version(),
        }
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    // See [`crate::text::root::internals`] — same gate, same reason.
    #![allow(dead_code)]
    use super::*;

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

    impl TestShape {
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
                Some(width) => request.bounded(width, self.halign, fit),
                None => request,
            }
        }
    }
}
