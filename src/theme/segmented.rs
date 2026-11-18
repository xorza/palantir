//! What a segmented radio group wears: its track and its segments.

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::button::ButtonTheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// Visuals for a [`RadioGroup`](crate::RadioGroup) in [`RadioLook::Segment`](crate::RadioLook::Segment):
/// one track with its choices flat on it and one pill that slides under the checked one, as
/// Apple's segmented control slides its thumb
/// ([Apple HIG, Segmented controls](https://developer.apple.com/design/human-interface-guidelines/segmented-controls)).
/// A label lights as the pill covers it: its ink is its unselected look's blended toward its
/// selected look's by the share of the segment under the pill, so the ink moves with the pill
/// and never on a clock of its own, which Emil Kowalski shows a timed color change cannot match
/// ([The magic of clip-path](https://emilkowal.ski/ui/the-magic-of-clip-path)). Refused: a
/// second copy of the labels clipped to the pill, which shapes every label twice.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[serde(deny_unknown_fields)]
pub struct SegmentedTheme {
    /// The track the segments sit on.
    pub track: Chrome,
    /// The track's inset around its segments.
    #[serde(deserialize_with = "crate::primitives::file::padding")]
    pub track_padding: Spacing,
    /// The pill under the checked segment, one for the track, which slides from segment to
    /// segment.
    pub pill: Chrome,
    /// How the pill moves to a newly checked segment. While motion is reduced it fades in at the
    /// new segment under this spec instead, as reduced-motion guidance swaps a slide for a fade
    /// ([Smashing Magazine](https://www.smashingmagazine.com/2020/09/design-reduced-motion-sensitivities));
    /// [`AnimationSpec::SNAP`] neither slides nor fades.
    #[serde(default, skip_serializing_if = "AnimationSpec::is_instant")]
    #[animate(snap)]
    pub slide: AnimationSpec,
    /// Gap between a segment's icon and its label.
    #[serde(deserialize_with = "crate::primitives::file::length")]
    pub gap: f32,
    /// What a segment wears over the pill: `selected` the checked one, `unselected` the others,
    /// each in its four states. A label's ink blends between the two as the pill covers it, and
    /// each draws its own chrome on top of the pill: the stock `selected` chrome is clear, and
    /// disabled a veil that dims the pill.
    pub segment: ButtonTheme,
}

impl SegmentedTheme {
    /// The track at `element_mid`, the resting fill of a control, and the checked segment on a
    /// pill of the accent under `on_accent` ink, as a checked checkbox and an on switch fill with
    /// the accent; the pill slides on a critically damped spring that arrives in about 270 ms. A
    /// segment at rest is flat in `text_muted`; hovered it takes `element_strong`, and pressed the
    /// `window_bg` ground, before the release sends the pill there; checked, it draws nothing over
    /// the pill, and disabled it veils the pill under half the track's fill and halves its ink, so
    /// a disabled pick does not read as a live accent. Refused: a pill of `window_bg` under a hairline, which carried no accent; a
    /// raised pill of the `element` rungs, which reads raised in a dark scheme and sunk in a light
    /// one, where the rungs step darker.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let track_padding = m.space.xxs;
        let corners = Corners::all((m.radius.control - track_padding).max(0.0));
        let look = |fill: RgbaF32, ink: RgbaF32| WidgetLook {
            chrome: Chrome::rounded(fill, corners),
            text: TextStyleOverrides::NONE.with_color(ink),
        };
        let on_pill = look(RgbaF32::TRANSPARENT, p.on_accent);
        let rest = look(RgbaF32::TRANSPARENT, p.text_muted);
        Self {
            track: Chrome::rounded(p.element_mid, Corners::all(m.radius.control)),
            track_padding: Spacing::all(track_padding),
            pill: Chrome::rounded(p.accent, corners),
            slide: stock_slide(),
            gap: m.space.xs,
            segment: ButtonTheme {
                selected: StatefulLook {
                    normal: on_pill.clone(),
                    hovered: on_pill.clone(),
                    active: on_pill,
                    disabled: look(
                        p.element_mid.with_alpha8(HALF),
                        p.on_accent.with_alpha8(HALF),
                    ),
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
                    press: None,
                },
            },
        }
    }
}

/// Half alpha on the 8-bit grid a theme file writes, the stock disabled pick's veil and ink.
const HALF: u8 = 128;

/// The stock pill's motion: a critically damped spring, `x'' + 49·x' + 600·x = 0`, damping
/// `2·√600 ≈ 48.99`, which arrives within 1 % of a segment in about 270 ms without overshoot from
/// rest and keeps its velocity when a pick retargets it mid-way, as SwiftUI's `snappy` spring
/// moves a picker ([WWDC23, Animate with springs](https://developer.apple.com/videos/play/wwdc2023/10158/)).
/// Refused: a duration, which takes one time for any distance and restarts from rest when
/// retargeted, a stall a moving pill shows.
fn stock_slide() -> AnimationSpec {
    AnimationSpec::spring(600.0, 49.0)
}
