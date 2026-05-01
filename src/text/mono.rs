use super::TextMeasure;
use crate::primitives::Size;

/// Deterministic placeholder metric: every glyph is `font_size_px * 0.5` wide
/// and the line is `font_size_px` tall. Used as the default in [`Ui`] so the
/// engine can be exercised in tests without bundling a font; layout tests pin
/// pixel values that depend on this exact formula.
///
/// At the historical 16 px font size this is the prior 8 px/char × 16 px line
/// height the engine was hard-coded to before the [`TextMeasure`] seam
/// existed. Wrapping is approximated by simple character-count division —
/// good enough to keep `Hug` sizing sane for monospace placeholders.
///
/// [`Ui`]: crate::Ui
#[derive(Clone, Copy, Debug, Default)]
pub struct MonoMeasure;

impl MonoMeasure {
    pub const fn new() -> Self {
        Self
    }
}

impl TextMeasure for MonoMeasure {
    fn measure(&mut self, text: &str, font_size_px: f32, max_width_px: Option<f32>) -> Size {
        if text.is_empty() {
            return Size::ZERO;
        }
        let glyph_w = font_size_px * 0.5;
        let line_h = font_size_px;
        let total_chars = text.chars().count() as f32;
        let unbroken_w = total_chars * glyph_w;

        match max_width_px {
            None => Size::new(unbroken_w, line_h),
            Some(max) if max >= unbroken_w => Size::new(unbroken_w, line_h),
            Some(max) => {
                let chars_per_line = (max / glyph_w).floor().max(1.0);
                let lines = (total_chars / chars_per_line).ceil().max(1.0);
                Size::new((chars_per_line * glyph_w).min(unbroken_w), lines * line_h)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        let mut m = MonoMeasure;
        assert_eq!(m.measure("", 16.0, None), Size::ZERO);
    }

    #[test]
    fn unbroken_matches_legacy_placeholder() {
        // Pre-trait code used `chars * 8.0` × `16.0` at the implicit 16 px size.
        let mut m = MonoMeasure;
        assert_eq!(m.measure("Hi", 16.0, None), Size::new(16.0, 16.0));
        assert_eq!(m.measure("hello!!", 16.0, None), Size::new(56.0, 16.0));
    }

    #[test]
    fn wraps_when_max_width_below_unbroken() {
        let mut m = MonoMeasure;
        // 8 chars × 8 px = 64 unbroken; max 32 → 4 chars/line, 2 lines.
        let s = m.measure("12345678", 16.0, Some(32.0));
        assert_eq!(s, Size::new(32.0, 32.0));
    }
}
