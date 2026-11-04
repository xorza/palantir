//! What a text field wears in each of its four states, plus the caret and
//! selection colors that have no state.

use crate::animation::animation_spec::AnimationSpec;
use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::{RgbaF32, TEXT_CONTRAST};
use crate::primitives::paint::stroke::Stroke;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use glam::Vec2;

/// Four-state TextEdit theme: a [`StatefulLook`] where `active` is **focused**,
/// with disabled > active > hovered > normal precedence. The default `hovered`
/// equals `normal`, so hover feedback is opt-in.
///
/// State-independent fields (`caret`, `caret_width`, `placeholder`,
/// `selection`, `padding`, `margin`) are flat on the theme. `padding`/`margin`
/// apply when the builder didn't set them; an explicit zero overrides.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_anim_derive::Lerp,
    palantir_anim_derive::TextSlots,
)]
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
    /// caret's prefix-x.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub caret_width: f32,
    /// Selection highlight fill, a wash behind the selected glyphs.
    pub selection: RgbaF32,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl TextEditTheme {
    /// Where a field must put its own corner for a run measuring `text` to come out
    /// centered on `at`.
    ///
    /// Keeps a value from jumping when it becomes editable: a field is a box
    /// *around* the run, so placing it at the run's corner offsets the glyphs by
    /// padding and stroke.
    ///
    /// Valid for a field that **hugs its content, centers it, and holds one line**,
    /// as an in-place edit does; wider fields spend alignment the theme can't know.
    ///
    /// The run's width cancels, and so does the caret's reserve, which the field hugs
    /// with the run and splits around centered glyphs: they start one caret width past
    /// the inner rect's leading edge.
    pub fn corner_centering(&self, text: Size, at: Vec2) -> Vec2 {
        // The inner rect sits inside the ring, as `Tree::open_node` folds it. Off `normal`: the
        // width is one number across states (see [`TextEditTheme::from_parts`]).
        let [left, top, ..] = self
            .looks
            .normal
            .chrome
            .fold_padding(self.defaults.padding)
            .to_array();
        at - Vec2::new(text.w, text.h) * 0.5 - Vec2::new(left + self.caret_width, top)
    }

    /// A field whose stroke changes color on focus but never width.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let radius = Corners::all(m.radius.control);
        // Constant stroke width across states: `Tree::open_node` folds it into
        // padding, so a change would shift the inner rect and jitter the text on
        // focus. The hairline a button wears, so a field and a button beside it
        // share one height, and the edge lands on whole pixels at 1x.
        let stroke_w = m.stroke.hairline;
        let normal_chrome = Chrome::rounded(p.element_mid, radius)
            .with_border(Stroke::new(p.border_soft(), stroke_w));
        let focused_chrome = Chrome::rounded(p.element_mid, radius)
            .with_border(Stroke::new(p.border_focused, stroke_w));
        let disabled_chrome =
            Chrome::rounded(p.element, radius).with_border(Stroke::new(p.border_soft(), stroke_w));
        let selection = selection_wash(p);
        // `hovered` defaults to `normal`: no hover feedback out of the box.
        let normal = WidgetLook {
            chrome: normal_chrome,
            text: TextStyleOverrides::NONE,
        };
        Self {
            looks: StatefulLook {
                hovered: normal.clone(),
                normal,
                active: WidgetLook {
                    chrome: focused_chrome,
                    text: TextStyleOverrides::NONE,
                },
                disabled: WidgetLook {
                    chrome: disabled_chrome,
                    text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                },
            },
            placeholder: p.text_muted,
            caret: p.text,
            caret_width: m.stroke.fine,
            selection,
            defaults: SlotDefaults {
                padding: Spacing::xy(m.space.sm, m.space.xs),
                margin: Spacing::ZERO,
                transition: AnimationSpec::SNAP,
            },
        }
    }

    /// The built-in look of `scheme`: its built-in palette over [`Metrics::DEFAULT`].
    pub fn stock(scheme: ColorScheme) -> Self {
        Self::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT)
    }

    /// This theme with no padding, margin or chrome in any state, for a field that reads as the
    /// text it edits: a rename in place, a search field on a panel. Its `Hug` height is the text's
    /// line, so a row keeps its height when such a field takes a label's place, as an
    /// [`EditableLabel`](crate::EditableLabel) does. The caret, the selection and the text keep
    /// this theme's.
    #[must_use]
    pub fn flat(&self) -> Self {
        let mut flat = Self {
            defaults: SlotDefaults {
                padding: Spacing::ZERO,
                margin: Spacing::ZERO,
                ..self.defaults
            },
            ..self.clone()
        };
        for look in flat.looks.each_mut() {
            look.chrome = Chrome::NONE;
        }
        flat
    }
}

impl ThemeSlot for TextEditTheme {
    type Pick = ();

    /// `active` = focused. Disabled beats focused, focused beats hovered.
    #[inline(always)]
    fn look(&self, response: &ResponseState, _pick: ()) -> &WidgetLook {
        self.looks.pick(response, Engagement::Focused)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}

/// The most of the accent, in 255ths, a selection wash takes, so a wash stays a tint over the
/// field under any accent.
const SELECTION_MAX: u8 = 61;

/// The selection wash: the accent at the most, in 255ths and at most [`SELECTION_MAX`], that keeps
/// the text at WCAG 4.5:1 over a field's `element_mid` washed with it, blended in linear light as
/// the composite blends (`theme/tests/floors.rs` holds every palette to it). A light accent
/// under light ink takes less, as more of it would lift the wash toward the ink.
fn selection_wash(p: &Palette) -> RgbaF32 {
    let reads = |a8: u8| {
        let a = p.accent.with_alpha8(a8).a;
        p.text.contrast_ratio(p.element_mid.mix(p.accent, a)) >= TEXT_CONTRAST
    };
    let a8 = (0..=SELECTION_MAX).rev().find(|&a8| reads(a8)).unwrap_or(0);
    p.accent.with_alpha8(a8)
}
