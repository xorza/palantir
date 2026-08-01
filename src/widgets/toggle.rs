use crate::layout::types::align::{Align, VAlign};
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::TextInput;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::text::Text;
use crate::widgets::theme::Theme;
use crate::widgets::theme::WidgetTheme;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::widget::WidgetEntry;

/// What [`toggle_row`] needs from its caller beyond the entry, the
/// label, and the indicator body.
///
/// The theme is passed unresolved (`style` + `slot`) rather than as
/// picked values: `toggle_row` hands both to [`resolve_look`], which is
/// where every widget in the crate turns a theme bundle into a painted
/// look. Callers still read the *geometry* scalars off the theme
/// themselves — they need them to size `boxed` and to paint the
/// indicator — but they no longer pick or animate.
#[derive(Debug)]
pub(crate) struct ToggleChrome<'a> {
    /// Per-instance override from the builder. `None` reads
    /// `slot(&ui.theme)`.
    pub(crate) style: Option<&'a ToggleTheme>,
    /// Which [`Theme`] field the override falls back to. The three
    /// toggles share a theme *type* but not a *slot* — restyling
    /// `checkbox` must leave `radio` and `switch` alone — so the
    /// fallback can't be hard-coded here.
    pub(crate) slot: fn(&Theme) -> &ToggleTheme,
    /// Checked / selected / on. Selects which of the theme's two look
    /// packs the four-state pick runs inside; reaches `resolve_look` as
    /// [`ToggleTheme`]'s `Mode`.
    pub(crate) on: bool,
    /// The box/track child recorded before the label, already sized and
    /// in its layout mode — a square leaf for `Checkbox`/`RadioButton`,
    /// a wide `Canvas` for `Switch`'s track. `toggle_row` only stamps
    /// the id (`<row>.with("box")`) and the resolved chrome onto it.
    pub(crate) boxed: Node,
    /// Corner radius forced onto the box chrome, overriding whatever
    /// radius the theme stored. The radio pip and the switch track must
    /// read as pills however they are re-themed; `None` keeps the
    /// theme's own corners (checkbox).
    pub(crate) pill: Option<f32>,
}

/// Shared `HStack [box, label]` scaffolding behind [`crate::Checkbox`],
/// [`crate::RadioButton`], and [`crate::Switch`]. The three differ only
/// in the toggle semantics (resolved by the caller before this runs),
/// the box child, and what `body` paints inside it. Everything
/// structural — the themed look resolution, the row gap /
/// cross-centering, the box chrome, the label leaf — lives here.
///
/// The row `HStack` node (sense + salt already set) rides in
/// `entry.widget`. `body` runs inside the box child and is handed the
/// box's resolved chrome: `Switch` measures its knob inset against the
/// *animating* stroke width, which is why the background is passed in
/// rather than re-derived from the theme.
pub(crate) fn toggle_row<'ui, 'text>(
    ui: &'ui mut Ui,
    mut entry: WidgetEntry,
    chrome: ToggleChrome<'_>,
    label: TextInput<'text>,
    body: impl FnOnce(&mut Ui, &Background),
) -> Response<'ui> {
    let id = entry.widget.id();
    let row_gap = chrome
        .style
        .unwrap_or_else(|| (chrome.slot)(&ui.theme))
        .row_gap;
    let mut look = WidgetTheme::resolve(
        ui,
        id,
        &mut entry.widget.node,
        &entry.state,
        chrome.on,
        chrome.style,
        chrome.slot,
    );
    if let Some(radius) = chrome.pill {
        look.background.corners = Corners::all(radius);
    }

    entry.widget.node.gaps.set_gap(row_gap);
    entry.widget.node.child_align = Align::v(VAlign::Center);

    entry.widget.record(ui, None, |ui| {
        ui.widget(chrome.boxed.id(id.with("box")))
            .record(ui, Some(&look.background), |ui| body(ui, &look.background));

        if !label.is_empty() {
            Text::new(label)
                .id(id.with("label"))
                .style(&look.text)
                .text_align(Align::v(VAlign::Center))
                .show(ui);
        }
    });

    entry.into_response(ui)
}

#[cfg(test)]
mod tests {
    use crate::primitives::spacing::Spacing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::scene::tree::node::NodeId;
    use crate::ui::harness::UiHarness;
    use crate::widgets::checkbox::Checkbox;
    use crate::widgets::radio::RadioButton;
    use crate::widgets::switch::Switch;
    use glam::UVec2;

    /// All three toggles resolve their box through `resolve_look` now,
    /// so [`crate::ToggleTheme`]'s `padding` / `margin` reach the row and
    /// an explicit builder value still wins — the contract `Button` and
    /// `TextEdit` already had. Before the shared resolver the toggle
    /// themes carried no spacing at all, so a toggle row was the one
    /// chrome-box widget an app couldn't space from its theme.
    ///
    /// One test over all three because they are now one code path: a
    /// regression that reached only `Switch` would mean `Switch` had
    /// stopped sharing it.
    #[test]
    fn theme_spacing_reaches_every_toggle_row_and_explicit_wins() {
        // Asymmetric, and different from each other, so neither a
        // padding/margin swap nor an axis swap can read as a pass.
        let padding = Spacing::xy(7.0, 5.0);
        let margin = Spacing::xy(3.0, 11.0);

        #[track_caller]
        fn check(
            label: &str,
            h: &UiHarness,
            nodes: [NodeId; 2],
            padding: Spacing,
            margin: Spacing,
        ) {
            let layouts = h.ui.forest.trees[Layer::Main].records.layout();
            let explicit = layouts[nodes[0].idx()];
            let inherited = layouts[nodes[1].idx()];
            assert_eq!(explicit.padding, Spacing::ZERO, "{label}: explicit padding");
            assert_eq!(explicit.margin, Spacing::ZERO, "{label}: explicit margin");
            assert_eq!(inherited.padding, padding, "{label}: theme padding");
            assert_eq!(inherited.margin, margin, "{label}: theme margin");
        }

        let mut h = UiHarness::new(UVec2::new(400, 300));
        for slot in [
            &mut h.ui.theme.checkbox,
            &mut h.ui.theme.radio,
            &mut h.ui.theme.switch,
        ] {
            slot.padding = padding;
            slot.margin = margin;
        }

        let (mut a, mut b) = (false, false);
        let rows = h.frame_value(|ui| {
            [
                Checkbox::new(&mut a)
                    .id(WidgetId::from_hash("cb-explicit"))
                    .padding(Spacing::ZERO)
                    .margin(Spacing::ZERO)
                    .show(ui)
                    .node(),
                Checkbox::new(&mut b)
                    .id(WidgetId::from_hash("cb-inherited"))
                    .show(ui)
                    .node(),
            ]
        });
        check("Checkbox", &h, rows, padding, margin);

        let (mut c, mut d) = (0_u8, 0_u8);
        let rows = h.frame_value(|ui| {
            [
                RadioButton::new(&mut c, 1)
                    .id(WidgetId::from_hash("rb-explicit"))
                    .padding(Spacing::ZERO)
                    .margin(Spacing::ZERO)
                    .show(ui)
                    .node(),
                RadioButton::new(&mut d, 1)
                    .id(WidgetId::from_hash("rb-inherited"))
                    .show(ui)
                    .node(),
            ]
        });
        check("RadioButton", &h, rows, padding, margin);

        let (mut e, mut f) = (false, false);
        let rows = h.frame_value(|ui| {
            [
                Switch::new(&mut e)
                    .id(WidgetId::from_hash("sw-explicit"))
                    .padding(Spacing::ZERO)
                    .margin(Spacing::ZERO)
                    .show(ui)
                    .node(),
                Switch::new(&mut f)
                    .id(WidgetId::from_hash("sw-inherited"))
                    .show(ui)
                    .node(),
            ]
        });
        check("Switch", &h, rows, padding, margin);
    }
}
