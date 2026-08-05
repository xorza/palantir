use crate::input::response::ResponseState;
use crate::layout::types::align::{Align, VAlign};
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::TextInput;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;
use crate::widgets::response::Response;
use crate::widgets::text::Text;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;
use crate::widgets::widget::Widget;

/// What [`toggle_row`] needs from its caller beyond the entry, the
/// label, and the indicator body.
///
/// The theme arrives **resolved**. Each toggle reads its own slot
/// (`theme.checkbox` / `theme.radio` / `theme.switch`) — they share a
/// theme *type* but not a *slot*, and only the caller knows which is
/// its own — so the caller is also where [`WidgetTheme::resolve`] runs,
/// beside the geometry scalars it already copies off that same slot.
/// What crosses into here is owned, which is what keeps the shared
/// scaffolding out of the business of naming theme fields.
#[derive(Debug)]
pub(crate) struct ToggleChrome {
    /// The picked, animated look for this response and on/off state.
    pub(crate) look: AnimatedLook,
    /// Gap between the box and the label, off the same slot as `look`.
    pub(crate) row_gap: f32,
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
/// `widget`, its probed response in `probe`. `body` runs inside the box
/// child and is handed the
/// box's resolved chrome: `Switch` measures its knob inset against the
/// *animating* stroke width, which is why the background is passed in
/// rather than re-derived from the theme.
pub(crate) fn toggle_row<'ui, 'text>(
    ui: &'ui mut Ui,
    mut widget: Widget,
    response: ResponseState,
    chrome: ToggleChrome,
    label: TextInput<'text>,
    body: impl FnOnce(&mut Ui, &Background),
) -> Response<'ui> {
    let id = widget.id();
    let ToggleChrome {
        mut look,
        row_gap,
        boxed,
        pill,
    } = chrome;
    if let Some(radius) = pill {
        look.background.corners = Corners::all(radius);
    }

    widget.node.gaps.set_gap(row_gap);
    widget.node.child_align = Align::v(VAlign::Center);

    widget.record(ui, None, |ui| {
        ui.widget(boxed.id(id.with("box")))
            .record(ui, Some(&look.background), |ui| body(ui, &look.background));

        if !label.is_empty() {
            Text::new(label)
                .id(id.with("label"))
                .style(&look.text)
                .text_align(Align::v(VAlign::Center))
                .show(ui);
        }
    });

    Response::eager(id, ui, response)
}

#[cfg(test)]
mod tests {
    use crate::primitives::spacing::Spacing;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::scene::tree::record::NodeId;
    use crate::ui::harness::UiHarness;
    use crate::widgets::checkbox::Checkbox;
    use crate::widgets::radio::RadioButton;
    use crate::widgets::switch::Switch;
    use glam::UVec2;

    /// All three toggles resolve their box through `WidgetTheme::resolve`,
    /// so [`crate::ToggleTheme`]'s `padding` / `margin` reach the row and
    /// an explicit builder value still wins — the same contract `Button`
    /// and `TextEdit` hold to.
    ///
    /// One test over all three because they are now one code path: a
    /// regression that reached only `Switch` would mean `Switch` had
    /// stopped sharing it.
    ///
    /// Each toggle gets its **own** spacing, so this also pins which slot
    /// each one reads. `toggle_row` is shared but the slots are not —
    /// restyling `checkbox` must leave `radio` and `switch` alone — and the
    /// three now name their slot at their own `WidgetTheme::resolve` call.
    /// Writing one value to all three slots could not tell them apart, so a
    /// toggle reading its neighbour's slot passed.
    #[test]
    fn theme_spacing_reaches_every_toggle_row_and_explicit_wins() {
        // Asymmetric, and different from each other, so neither a
        // padding/margin swap nor an axis swap can read as a pass — and
        // distinct per toggle, so nor can a slot mix-up.
        let spacing = |n: f32| (Spacing::xy(n, n + 2.0), Spacing::xy(n + 4.0, n + 6.0));
        let (cb_padding, cb_margin) = spacing(7.0);
        let (rb_padding, rb_margin) = spacing(23.0);
        let (sw_padding, sw_margin) = spacing(41.0);

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
        for (slot, (padding, margin)) in [
            (&mut h.ui.theme.checkbox, (cb_padding, cb_margin)),
            (&mut h.ui.theme.radio, (rb_padding, rb_margin)),
            (&mut h.ui.theme.switch, (sw_padding, sw_margin)),
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
        check("Checkbox", &h, rows, cb_padding, cb_margin);

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
        check("RadioButton", &h, rows, rb_padding, rb_margin);

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
        check("Switch", &h, rows, sw_padding, sw_margin);
    }
}
