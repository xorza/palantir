use crate::animation::AnimSpec;
use crate::input::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;

/// Three-state TextEdit theme. The leaf type ([`WidgetLook`]) lives
/// next to it; widget reads `theme.{normal,focused,disabled}` based
/// on `Element::disabled` and focus. Use [`Self::pick`] to select.
///
/// State-independent fields (`caret`, `caret_width`, `placeholder`,
/// `selection`, `padding`, `margin`) live flat on the theme — they
/// aren't state-varying in any plausible v1.x design.
///
/// `padding`/`margin` apply when the user didn't call
/// `.padding(...)` / `.margin(...)` on the builder. The "user didn't
/// override" check is `element.padding == Spacing::ZERO` — so if you
/// want a TextEdit with no padding while the theme has padding, set a
/// custom theme rather than passing zero.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TextEditTheme {
    pub normal: WidgetLook,
    pub focused: WidgetLook,
    pub disabled: WidgetLook,
    pub placeholder: Color,
    pub caret: Color,
    /// Width of the caret rect in logical px. The caret is painted as
    /// a thin Overlay rect at the caret's prefix-x; one pixel reads as
    /// a hairline, two as a chunkier i-beam. Default 1.5 px.
    pub caret_width: f32,
    /// Selection highlight fill. Unused in v1 (no selection ops yet)
    /// but kept on the theme so enabling selection later doesn't
    /// require a theme migration.
    pub selection: Color,
    /// Default padding inside the editor (around the buffer text).
    /// Applied at `show()` time when the builder hasn't set padding.
    pub padding: Spacing,
    /// Default margin around the editor.
    pub margin: Spacing,
    /// Spec applied to fill/stroke/text transitions between states.
    /// Default `None` — animation is opt-in (matches `ButtonTheme`).
    /// Round-trips through serde so theme files configure motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
}

impl TextEditTheme {
    /// Pick the visual state. Disabled wins over focused; otherwise
    /// normal. `state.disabled` is the cascaded ancestor-or-self flag
    /// — caller can merge `state.disabled |= element.disabled` for
    /// lag-free response to its own self-toggle (mirrors Button).
    pub fn pick(&self, state: ResponseState) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if state.focused {
            &self.focused
        } else {
            &self.normal
        }
    }
}

impl Default for TextEditTheme {
    fn default() -> Self {
        let radius = Corners::all(4.0);
        // Palette BORDER is ~2% above SURFACE — invisible. Derive edge from TEXT_MUTED alpha.
        let m = palette::TEXT_MUTED;
        let edge = m.with_alpha(0.18);
        let normal_bg = Background {
            fill: palette::ELEM_HOVER.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius,
            shadow: Shadow::NONE,
        };
        let focused_bg = Background {
            fill: palette::ELEM_HOVER.into(),
            stroke: Stroke::solid(palette::BORDER_FOCUSED, 1.5),
            radius,
            shadow: Shadow::NONE,
        };
        let disabled_bg = Background {
            fill: palette::ELEM.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius,
            shadow: Shadow::NONE,
        };
        // Selection = accent at ~25% alpha — readable wash that doesn't
        // obscure the glyphs underneath.
        let acc = palette::ACCENT;
        let selection = acc.with_alpha(0.25);
        Self {
            normal: WidgetLook {
                background: Some(normal_bg),
                text: None,
            },
            focused: WidgetLook {
                background: Some(focused_bg),
                text: None,
            },
            disabled: WidgetLook {
                background: Some(disabled_bg),
                text: Some(TextStyle::default().with_color(palette::TEXT_DISABLED)),
            },
            placeholder: palette::TEXT_MUTED,
            caret: palette::TEXT,
            caret_width: 1.5,
            selection,
            padding: Spacing::xy(5.0, 3.0),
            margin: Spacing::ZERO,
            anim: None,
        }
    }
}
