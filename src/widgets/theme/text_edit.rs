use crate::animation::AnimSpec;
use crate::input::response::ResponseState;
use crate::primitives::approx::noop_f32;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;
use glam::Vec2;

/// Four-state TextEdit theme: a [`StatefulLook`] where `active` =
/// **focused** (the editor's engaged state), picked with the uniform
/// disabled > active > hovered > normal precedence. The default
/// `hovered` look equals `normal`, so hover feedback is opt-in.
///
/// State-independent fields (`caret`, `caret_width`, `placeholder`,
/// `selection`, `padding`, `margin`) live flat on the theme — they
/// aren't state-varying in any plausible v1.x design.
///
/// `padding`/`margin` apply when the user didn't call
/// `.padding(...)` / `.margin(...)` on the builder. Explicit zero spacing
/// overrides the theme like any other value.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TextEditTheme {
    /// The four per-state looks (`active` = focused). `flatten` keeps
    /// theme files flat (`[text_edit.active]`, not
    /// `[text_edit.looks.active]`).
    #[serde(flatten)]
    pub looks: StatefulLook,
    pub placeholder: Color,
    pub caret: Color,
    /// Width of the caret rect in logical px. The caret is painted as
    /// a thin Overlay rect at the caret's prefix-x; one pixel reads as
    /// a hairline, two as a chunkier i-beam. Default 1.5 px.
    pub caret_width: f32,
    /// Selection highlight fill, painted as a wash behind the selected
    /// glyphs (see `TextEdit::show`).
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
    /// `placeholder` / `caret` / `selection` are bare `Color`s, not
    /// `TextStyle`s — they take their size from the resolved look.
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            looks,
            placeholder: _,
            caret: _,
            caret_width: _,
            selection: _,
            padding: _,
            margin: _,
            anim: _,
        } = self;
        looks.for_each_text(f);
    }

    /// Pick the visual state: `active` = focused. Disabled wins over
    /// focused, focused over hovered; otherwise normal.
    /// `state.disabled` is the cascaded ancestor-or-self flag —
    /// caller can merge `state.disabled |= node.disabled` for
    /// lag-free response to its own self-toggle (mirrors Button).
    #[inline(always)]
    pub fn pick(&self, state: &ResponseState) -> &WidgetLook {
        self.looks.pick(state, state.focused)
    }

    /// Where a field must put its own corner for a run measuring `text` to come
    /// out centred on `at`.
    ///
    /// **What keeps a value from moving as it becomes editable.** An
    /// application that edits something in place draws it, then stands a field
    /// where it was — and a field is a box *around* a run rather than the run
    /// itself, so a corner set at the run's own corner lays the glyphs a
    /// padding, a stroke and a caret's room off the pixels they were read at.
    /// Which is a jump on the frame the field opens, in a direction nothing
    /// about the value explains.
    ///
    /// The answer for a field that **hugs its content, centres it, and holds
    /// one line** — which is what an in-place edit is. Anything wider has room
    /// its alignment spends rather than its theme, and a theme could not say
    /// where that went.
    ///
    /// The run's own width cancels — the field centres the same text on the
    /// same point — but the caret's room does not: it is reserved at the
    /// trailing edge alone and the run is centred in what is left, so the
    /// glyphs sit half a caret to the leading side of the box's own middle.
    pub fn corner_centring(&self, text: Size, at: Vec2) -> Vec2 {
        let [left, top, ..] = self.padding.as_array();
        // `Tree::open_node` folds the chrome's stroke into the padding, so the
        // inner rect a run is laid in sits inside the ring as well — and
        // `TextEdit::show` mirrors that fold rather than reading the node back.
        //
        // Off `normal`, and safe to be: the width is one number across all four
        // states so that focus changes a field's colour without moving its
        // text — see [`TextEditTheme::from_palette`].
        let width = self.looks.normal.background.stroke.width;
        // On the width alone, like `TextEdit::show`'s own guard — a stroke the
        // colour makes invisible is still a stroke the fold makes room for, so
        // `Stroke::is_noop` is the wrong question here.
        let ring = if noop_f32(width) { 0.0 } else { width };
        at - Vec2::new(text.w, text.h) * 0.5
            - Vec2::new(left + ring + self.caret_width * 0.5, top + ring)
    }

    pub fn from_palette(p: &Palette) -> Self {
        let radius = Corners::all(4.0);
        // Stroke width stays constant across states — color is the
        // only thing that changes on focus. `Tree::open_node` folds
        // stroke width into padding so a width change between
        // normal/focused would shift the inner rect by half the
        // delta on each side, jittering the text the instant focus
        // lands. Picking 1.5 px gives focused its emphasis without
        // the layout shift.
        let stroke_w = 1.5;
        let normal_bg = Background::rounded(p.elem_hover, radius)
            .with_stroke(Stroke::solid(p.border_soft(), stroke_w));
        let focused_bg = Background::rounded(p.elem_hover, radius)
            .with_stroke(Stroke::solid(p.border_focused, stroke_w));
        let disabled_bg = Background::rounded(p.elem, radius)
            .with_stroke(Stroke::solid(p.border_soft(), stroke_w));
        // Selection = accent at ~25% alpha — readable wash that doesn't
        // obscure the glyphs underneath.
        let selection = p.accent.with_alpha(0.25);
        // `hovered` defaults to the `normal` look — editors don't give
        // hover feedback out of the box; the slot exists for themes
        // that want it.
        let normal = WidgetLook {
            background: normal_bg,
            text: None,
        };
        Self {
            looks: StatefulLook {
                hovered: normal.clone(),
                normal,
                active: WidgetLook {
                    background: focused_bg,
                    text: None,
                },
                disabled: WidgetLook {
                    background: disabled_bg,
                    text: Some(TextStyle::default().with_color(p.text_disabled)),
                },
            },
            placeholder: p.text_muted,
            caret: p.text,
            caret_width: 1.5,
            selection,
            padding: Spacing::xy(5.0, 3.0),
            margin: Spacing::ZERO,
            anim: None,
        }
    }
}

palette_default!(TextEditTheme);
