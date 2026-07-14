//! Shared damage-region policy used by CPU culling and backend scissors.

/// Physical-pixel padding around every damage scissor for antialiasing
/// fringes and glyph overhang.
pub(crate) const DAMAGE_AA_PADDING: u32 = 2;

/// Logical-pixel slack matching the backend's padded physical scissor.
pub(crate) fn damage_cull_margin(scale: f32) -> f32 {
    (DAMAGE_AA_PADDING as f32 + 1.0) / scale
}

#[cfg(test)]
mod tests {
    use super::damage_cull_margin;

    #[test]
    fn damage_cull_margin_scales_inversely() {
        assert_eq!(damage_cull_margin(1.0), 3.0);
        assert_eq!(damage_cull_margin(2.0), 1.5);
    }
}
