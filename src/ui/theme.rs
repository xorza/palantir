use crate::widgets::ButtonTheme;

/// Global theme. Aggregates per-widget themes plus rendering knobs that
/// apply tree-wide. Widgets opt in by reading from `Ui::theme`.
#[derive(Clone, Debug)]
pub struct Theme {
    pub button: ButtonTheme,
    /// RGB multiplier applied to fill/stroke/text colors of any node whose
    /// cumulative cascade is `disabled`. `1.0` = unchanged, `0.5` = half
    /// brightness. Alpha is preserved.
    pub disabled_dim: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            button: ButtonTheme::default(),
            disabled_dim: 0.5,
        }
    }
}
