//! What a disclosure header wears, and how far its body is inset.

use crate::input::response::response_state::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;
use crate::widgets::theme::widget_look::theme_slot::{SlotDefaults, ThemeSlot};
use glam::Vec2;
use std::f32::consts::FRAC_PI_2;

/// Visuals for [`crate::Expander`].
///
/// The header is a button in everything but name — the whole row is one
/// hit target — so it carries the same four-state look pack, and its
/// arrow takes the picked look's text colour rather than a slot of its
/// own.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExpanderTheme {
    /// Four-state look for the header row.
    pub looks: StatefulLook,
    /// Arrow bounding box in logical px.
    ///
    /// **Square, unless the angles below leave it unturned.** A quarter
    /// turn swaps the arrow's extents, so an oblong box clips it on one
    /// axis.
    pub arrow_size: Vec2,
    /// Stroke width of the arrow polyline.
    pub arrow_stroke: f32,
    /// Angle the arrow wears while the body is closed, in radians. The
    /// default quarter turn anticlockwise points it at the label, which
    /// is the disclosure triangle every file tree draws.
    pub arrow_closed_angle: f32,
    /// Angle the arrow wears while the body is open. The default leaves
    /// it upright, pointing down at what it revealed.
    ///
    /// Set the pair to `0.0` and `-PI` for the other convention — down
    /// when closed, up when open — which reads better for a column of
    /// sibling sections than for one disclosure.
    pub arrow_open_angle: f32,
    /// Gutter between the arrow and the label.
    pub gap: f32,
    /// How far the body is inset from the header's leading edge.
    pub indent: f32,
    /// Inset between the body's edges and its content.
    ///
    /// Named apart from [`SlotDefaults::padding`], which this bundle
    /// flattens: that one is the box default the header takes, and two
    /// fields of one name collide on the wire.
    pub body_padding: Spacing,
    /// Spacing and transition spec — see [`SlotDefaults`]. `anim` is
    /// `None` by default, so a reveal snaps until an application asks
    /// for the motion.
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl ExpanderTheme {
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            looks,
            arrow_size: _,
            arrow_stroke: _,
            arrow_closed_angle: _,
            arrow_open_angle: _,
            gap: _,
            indent: _,
            body_padding: _,
            defaults: _,
        } = self;
        looks.for_each_text(f);
    }

    /// The angle the arrow wears at `openness`, `0.0` closed through
    /// `1.0` open.
    pub(crate) fn arrow_angle(&self, openness: f32) -> f32 {
        let t = openness.clamp(0.0, 1.0);
        self.arrow_closed_angle + (self.arrow_open_angle - self.arrow_closed_angle) * t
    }

    pub fn from_palette(p: &Palette) -> Self {
        let radius = Corners::all(4.0);
        Self {
            looks: StatefulLook {
                normal: WidgetLook {
                    background: Background::NONE,
                    text: None,
                },
                hovered: WidgetLook {
                    background: Background::rounded(p.elem_mid, radius),
                    text: None,
                },
                active: WidgetLook {
                    background: Background::rounded(p.elem_strong, radius),
                    text: None,
                },
                disabled: WidgetLook {
                    background: Background::NONE,
                    text: Some(TextStyle::default().with_color(p.text_disabled)),
                },
            },
            arrow_size: Vec2::new(9.0, 9.0),
            arrow_stroke: 1.5,
            arrow_closed_angle: -FRAC_PI_2,
            arrow_open_angle: 0.0,
            gap: 8.0,
            indent: 17.0,
            body_padding: Spacing::new(0.0, 4.0, 0.0, 4.0),
            defaults: SlotDefaults {
                padding: Spacing::new(4.0, 4.0, 4.0, 4.0),
                margin: Spacing::ZERO,
                anim: None,
            },
        }
    }
}

impl Default for ExpanderTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}

impl ThemeSlot for ExpanderTheme {
    type Pick = ();

    fn look(&self, response: &ResponseState, _: ()) -> &WidgetLook {
        self.looks.pick(response, response.pressed())
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
