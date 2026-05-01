use crate::widgets::ButtonStyle;

/// Global theme. Holds button defaults plus rendering knobs that apply tree-
/// wide (e.g. how strongly to dim a disabled subtree). Widgets opt in by
/// reading from `Ui::theme`; the encoder reads `disabled_alpha` once per
/// frame to fade fill/stroke/text in disabled subtrees.
#[derive(Clone, Debug)]
pub struct ButtonTheme {
    pub button: ButtonStyle,
    /// RGB multiplier applied to fill/stroke/text colors of any node whose
    /// cumulative cascade is `disabled`. `1.0` = unchanged, `0.5` = half
    /// brightness. Alpha is preserved.
    pub disabled_dim: f32,
}

impl Default for ButtonTheme {
    fn default() -> Self {
        Self {
            button: ButtonStyle::default(),
            disabled_dim: 0.5,
        }
    }
}
