use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::configure::Configure;
use crate::widgets::radio::RadioButton;
use crate::widgets::switch::Switch;
use glam::UVec2;

/// All three toggles resolve their box through `WidgetTheme::plan`,
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
/// three name their slot exactly once, at their own `style` use.
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
    fn check(label: &str, h: &UiHarness, nodes: [NodeId; 2], padding: Spacing, margin: Spacing) {
        let layouts = h.ui.tree(Layer::Main).records.layout();
        let explicit = layouts[nodes[0].idx()];
        let inherited = layouts[nodes[1].idx()];
        assert_eq!(explicit.padding, Spacing::ZERO, "{label}: explicit padding");
        assert_eq!(explicit.margin, Spacing::ZERO, "{label}: explicit margin");
        assert_eq!(inherited.padding, padding, "{label}: theme padding");
        assert_eq!(inherited.margin, margin, "{label}: theme margin");
    }

    let mut h = UiHarness::new(UVec2::new(400, 300));
    let theme = h.ui.theme_mut();
    for (slot, (padding, margin)) in [
        (&mut theme.checkbox, (cb_padding, cb_margin)),
        (&mut theme.radio, (rb_padding, rb_margin)),
        (&mut theme.switch, (sw_padding, sw_margin)),
    ] {
        slot.defaults.padding = padding;
        slot.defaults.margin = margin;
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
