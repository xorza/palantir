//! What a text field wears in each of its four states, plus the caret and
//! selection colours that have no state.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::stateful_look::StatefulLook;
use crate::widget_core::widget_look::theme_slot::{SlotDefaults, ThemeSlot};
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyleOverrides;
use glam::Vec2;

/// Four-state TextEdit theme: a [`StatefulLook`] where `active` is **focused**,
/// with disabled > active > hovered > normal precedence. The default `hovered`
/// equals `normal`, so hover feedback is opt-in.
///
/// State-independent fields (`caret`, `caret_width`, `placeholder`,
/// `selection`, `padding`, `margin`) are flat on the theme. `padding`/`margin`
/// apply when the builder didn't set them; an explicit zero overrides.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TextEditTheme {
    /// The four per-state looks (`active` = focused). `flatten` keeps theme files
    /// flat (`[text_edit.active]`).
    #[serde(flatten)]
    pub looks: StatefulLook,
    /// Ink for the placeholder text an empty field shows.
    pub placeholder: RgbaF32,
    /// Ink for the caret.
    pub caret: RgbaF32,
    /// Width of the caret rect in logical px, painted as a thin Overlay rect at the
    /// caret's prefix-x. Default 1.5 px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub caret_width: f32,
    /// Selection highlight fill, a wash behind the selected glyphs.
    pub selection: RgbaF32,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl TextEditTheme {
    /// `placeholder` / `caret` / `selection` are bare `RgbaF32`s that take their
    /// size from the resolved look. Destructured so a new field fails to compile
    /// here; see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            looks,
            placeholder: _,
            caret: _,
            caret_width: _,
            selection: _,
            defaults: _,
        } = self;
        looks.for_each_text(f);
    }

    /// Where a field must put its own corner for a run measuring `text` to come out
    /// centred on `at`.
    ///
    /// Keeps a value from jumping when it becomes editable: a field is a box
    /// *around* the run, so placing it at the run's corner offsets the glyphs by
    /// padding, stroke and caret room.
    ///
    /// Valid for a field that **hugs its content, centres it, and holds one line**,
    /// as an in-place edit does; wider fields spend alignment the theme can't know.
    ///
    /// The run's width cancels, but the caret's room does not: it is reserved at the
    /// trailing edge alone, so the glyphs sit half a caret to the leading side of
    /// the box's middle.
    pub fn corner_centering(&self, text: Size, at: Vec2) -> Vec2 {
        let [left, top, ..] = self.defaults.padding.as_array();
        // `Tree::open_node` folds the chrome's border into the padding, so the inner
        // rect sits inside the ring, and `TextEdit::show` mirrors that fold. Off
        // `normal`: the width is one number across states (see
        // [`TextEditTheme::from_palette`]).
        let ring = self.looks.normal.background.border_inset();
        at - Vec2::new(text.w, text.h) * 0.5
            - Vec2::new(left + ring + self.caret_width * 0.5, top + ring)
    }

    /// A field whose stroke changes colour on focus but never width.
    pub fn from_palette(p: &Palette) -> Self {
        let radius = Corners::all(4.0);
        // Constant stroke width across states: `Tree::open_node` folds it into
        // padding, so a change would shift the inner rect and jitter the text on
        // focus. 1.5 px gives focus emphasis without the shift.
        let stroke_w = 1.5;
        let normal_bg = Background::rounded(p.element_mid, radius)
            .with_border(Stroke::new(p.border_soft(), stroke_w));
        let focused_bg = Background::rounded(p.element_mid, radius)
            .with_border(Stroke::new(p.border_focused, stroke_w));
        let disabled_bg = Background::rounded(p.element, radius)
            .with_border(Stroke::new(p.border_soft(), stroke_w));
        // Selection = accent at ~25% alpha: a wash that leaves glyphs readable.
        let selection = p.accent.with_alpha(0.25);
        // `hovered` defaults to `normal`: no hover feedback out of the box.
        let normal = WidgetLook {
            background: normal_bg,
            text: TextStyleOverrides::NONE,
        };
        Self {
            looks: StatefulLook {
                hovered: normal.clone(),
                normal,
                active: WidgetLook {
                    background: focused_bg,
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    background: disabled_bg,
                    text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                },
            },
            placeholder: p.text_muted,
            caret: p.text,
            caret_width: 1.5,
            selection,
            defaults: SlotDefaults {
                padding: Spacing::xy(5.0, 3.0),
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }
}

impl ThemeSlot for TextEditTheme {
    type Pick = ();

    /// `active` = focused. Disabled beats focused, focused beats hovered.
    #[inline(always)]
    fn look(&self, response: &ResponseState, _pick: ()) -> &WidgetLook {
        self.looks.pick(response, response.focused)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}

impl Default for TextEditTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
