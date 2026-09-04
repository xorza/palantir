//! What a tab strip wears: the chip look packs, the selection cap, and
//! the band the chips sit on.

use crate::input::response::response_state::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;
use crate::widgets::theme::widget_look::theme_slot::{SlotDefaults, ThemeSlot};

/// Visuals for [`crate::TabStrip`], and so for [`crate::TabbedView`] and
/// every [`crate::DockView`] pane that records one.
///
/// One four-state look pack per selected state, so hover and press
/// resolve through the same [`StatefulLook::pick`] precedence as every
/// other widget. The selected chip additionally wears a cap along its
/// top edge — [`Self::accent`] while the strip holds focus,
/// [`Self::accent_idle`] while it does not, which is what makes one pane
/// read as "where actions go".
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TabsTheme {
    /// Look pack for the selected chip. Its fill defaults to the window
    /// ground, so the chip's bottom edge dissolves into the content
    /// below it.
    pub active: StatefulLook,
    /// Look pack for every other chip.
    pub inactive: StatefulLook,
    /// Selection cap on the focused strip.
    pub accent: RgbaF32,
    /// Selection cap on a strip that does not hold focus.
    pub accent_idle: RgbaF32,
    /// Cap breadth in logical px. The selected chip lifts its inner top
    /// inset by the same amount, so the cap adds no height and every
    /// label sits on the same line.
    pub accent_thickness: f32,
    /// The band behind the chips. [`Background::NONE`] by default: a
    /// strip reads from its own chips, and an application that wants a
    /// band under them is saying its chrome continues there — which is
    /// a fact about that application's surfaces, not about tabs.
    pub strip: Background,
    /// Inset between the band's edges and the chips.
    pub strip_padding: Spacing,
    /// Gutter between two chips.
    pub gap: f32,
    /// Hairline under the band, drawn only when
    /// [`Self::hline_thickness`] is set.
    pub hline: RgbaF32,
    /// Hairline breadth in logical px. `0.0` — the default — records no
    /// rule at all, so the chips meet the content below them directly.
    pub hline_thickness: f32,
    /// Chip corner radius. Applied to the top corners only — a chip
    /// meets the content below it square.
    pub corner: f32,
    /// Inset between a chip's edges and its label. Named apart from
    /// [`SlotDefaults::padding`], which this bundle flattens: that one
    /// is the box default the strip node takes, this one is the chip's
    /// own inner inset.
    pub chip_padding: Spacing,
    /// Trailing inset a chip takes in place of [`Self::chip_padding`]'s
    /// right one whenever something sits after the label — a badge, a
    /// close button, or both.
    ///
    /// Their own boxes already carry the breathing room the right inset
    /// exists to give a bare label, so charging both leaves a chip
    /// looking like it reserves a slot it does not have.
    pub trailing_inset: f32,
    /// Chip width floor, so a one-glyph label still reads as a tab.
    pub min_width: f32,
    /// Chip width ceiling. What lets a long title ellipsise instead of
    /// pushing its neighbours out of the band.
    pub max_width: f32,
    /// Look pack for the chip's close button.
    pub close: StatefulLook,
    /// Close button side in logical px.
    pub close_size: f32,
    /// Ink of the status dot.
    pub badge: RgbaF32,
    /// Status-dot diameter in logical px.
    pub badge_size: f32,
    /// Gutter between a chip's own children — its icon, label, badge
    /// and close button.
    pub label_gap: f32,
    /// Spacing and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl TabsTheme {
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            active,
            inactive,
            close,
            accent: _,
            accent_idle: _,
            accent_thickness: _,
            strip: _,
            strip_padding: _,
            gap: _,
            hline: _,
            hline_thickness: _,
            corner: _,
            chip_padding: _,
            trailing_inset: _,
            min_width: _,
            max_width: _,
            close_size: _,
            badge: _,
            badge_size: _,
            label_gap: _,
            defaults: _,
        } = self;
        active.for_each_text(f);
        inactive.for_each_text(f);
        close.for_each_text(f);
    }

    /// Pick the chrome+label look for this `(state, selected)` pair
    /// (`active` = pressed).
    pub fn pick(&self, state: &ResponseState, selected: bool) -> &WidgetLook {
        if selected {
            self.active.pick(state, state.pressed())
        } else {
            self.inactive.pick(state, state.pressed())
        }
    }

    /// The cap colour a strip paints under its selected chip.
    pub fn cap(&self, focused: bool) -> RgbaF32 {
        if focused {
            self.accent
        } else {
            self.accent_idle
        }
    }

    pub fn from_palette(p: &Palette) -> Self {
        let corner = 4.0;
        let top = Corners::top(corner);
        let inactive_text = Some(TextStyle::default().with_color(p.text_muted));
        let disabled_text = Some(TextStyle::default().with_color(p.text_disabled));
        let chip = |fill: RgbaF32, text: Option<TextStyle>| WidgetLook {
            background: Background::rounded(fill, top),
            text,
        };
        Self {
            active: StatefulLook {
                normal: chip(p.terminal_bg, None),
                hovered: chip(p.terminal_bg, None),
                active: chip(p.terminal_bg, None),
                disabled: chip(p.terminal_bg, disabled_text),
            },
            inactive: StatefulLook {
                normal: chip(p.elem_mid, inactive_text),
                hovered: chip(p.elem_strong, inactive_text),
                active: chip(p.elem_strong, None),
                disabled: chip(p.elem, disabled_text),
            },
            accent: p.accent,
            accent_idle: p.elem_strong,
            accent_thickness: 2.0,
            strip: Background::NONE,
            strip_padding: Spacing::new(6.0, 4.0, 6.0, 0.0),
            gap: 3.0,
            hline: p.border_soft(),
            hline_thickness: 0.0,
            corner,
            chip_padding: Spacing::new(10.0, 4.0, 10.0, 4.0),
            trailing_inset: 4.0,
            min_width: 48.0,
            max_width: 200.0,
            close: StatefulLook {
                normal: WidgetLook {
                    background: Background::NONE,
                    text: inactive_text,
                },
                hovered: WidgetLook {
                    background: Background::rounded(p.elem_strong, Corners::all(3.0)),
                    text: None,
                },
                active: WidgetLook {
                    background: Background::rounded(p.elem_strong, Corners::all(3.0))
                        .with_stroke(Stroke::solid(p.border_focused, 1.0)),
                    text: None,
                },
                disabled: WidgetLook {
                    background: Background::NONE,
                    text: disabled_text,
                },
            },
            close_size: 16.0,
            badge: p.accent,
            badge_size: 3.5,
            label_gap: 6.0,
            defaults: SlotDefaults {
                padding: Spacing::ZERO,
                margin: Spacing::ZERO,
                anim: None,
            },
        }
    }
}

palette_default!(TabsTheme);

impl ThemeSlot for TabsTheme {
    type Pick = bool;

    fn look(&self, response: &ResponseState, selected: bool) -> &WidgetLook {
        self.pick(response, selected)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
