//! What a segmented radio group wears: its track and its segments.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::button::ButtonTheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// Visuals for a [`RadioGroup`](crate::RadioGroup) in [`RadioLook::Segment`](crate::RadioLook::Segment):
/// one track with its choices flat on it and the checked one on a pill, as Apple's segmented
/// control and Radix's segmented control draw one
/// ([Apple HIG, Segmented controls](https://developer.apple.com/design/human-interface-guidelines/segmented-controls),
/// [Radix Segmented Control](https://www.radix-ui.com/themes/docs/components/segmented-control)).
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct SegmentedTheme {
    /// The track the segments sit on.
    pub track: Chrome,
    /// The track's inset around its segments.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub track_padding: Spacing,
    /// What a segment wears: `selected` the checked one, `unselected` the others, each in its four
    /// states, as a [`Button`](crate::Button) picks them.
    pub segment: ButtonTheme,
}

impl SegmentedTheme {
    /// The track at `element_mid`, the resting fill of a control, and the checked segment on a
    /// pill of `window_bg` with a soft hairline, so it reads as the ground showing through the
    /// track. A segment at rest is flat in `text_muted`; hovered it takes `element_strong`, and
    /// pressed the pill's ground, before the release moves the pill there. Refused: a raised pill
    /// of the `element` rungs, which reads raised in a dark scheme and sunk in a light one, where
    /// the rungs step darker.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let track_padding = m.space.xxs;
        let corners = Corners::all((m.radius.control - track_padding).max(0.0));
        let look = |fill: RgbaF32, ink: RgbaF32| WidgetLook {
            chrome: Chrome::rounded(fill, corners),
            text: TextStyleOverrides::NONE.with_color(ink),
        };
        let pill = WidgetLook {
            chrome: Chrome::rounded(p.window_bg, corners)
                .with_border(Stroke::new(p.border_soft(), m.stroke.hairline)),
            ..look(p.window_bg, p.text)
        };
        let rest = look(RgbaF32::TRANSPARENT, p.text_muted);
        Self {
            track: Chrome::rounded(p.element_mid, Corners::all(m.radius.control)),
            track_padding: Spacing::all(track_padding),
            segment: ButtonTheme {
                selected: StatefulLook {
                    normal: pill.clone(),
                    hovered: pill.clone(),
                    active: pill.clone(),
                    disabled: WidgetLook {
                        text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                        ..pill
                    },
                },
                unselected: StatefulLook {
                    normal: rest,
                    hovered: look(p.element_strong, p.text),
                    active: look(p.window_bg, p.text),
                    disabled: look(RgbaF32::TRANSPARENT, p.text_disabled),
                },
                defaults: SlotDefaults {
                    padding: Spacing::xy(m.space.sm, m.space.xxs),
                    margin: Spacing::ZERO,
                    transition: m.transition,
                },
            },
        }
    }
}
