use crate::animation::animation_spec::AnimationSpec;
use crate::host::harness::Harness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::sense::Sense;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::theme::Theme;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::widget_core::configure::Configure;
use crate::widgets::color_swatch::ColorSwatch;
use glam::UVec2;

/// A chip is a `swatch_size` square from the theme or the bundle `style` names. Opaque, it records one shape, the color. Translucent, the checker goes behind first: the light fill, then a dark cell on every other square of the `checker_cell` grid (24 / 6 = 4 cells a side, two dark per row over four rows).
#[test]
fn a_chip_is_a_themed_square_with_a_checker_behind_translucency() {
    let id = WidgetId::from_hash("chip");
    let styled = ColorPickerTheme {
        swatch_size: 24.0,
        checker_cell: 6.0,
        ..Theme::stock(ColorScheme::Dark).color_picker
    };
    let stock = Theme::stock(ColorScheme::Dark).color_picker.swatch_size;
    assert_ne!(stock, 24.0, "premise: the style moves the side");
    for (style, color, side, shapes) in [
        (None, RgbaF32::srgb(0.2, 0.4, 0.6), stock, 1),
        (Some(&styled), RgbaF32::srgb(0.2, 0.4, 0.6), 24.0, 1),
        (
            Some(&styled),
            RgbaF32::srgba(0.2, 0.4, 0.6, 0.5),
            24.0,
            1 + 4 * 2 + 1,
        ),
    ] {
        let mut h = Harness::mono(UVec2::new(100, 100));
        let node = h.frame_value(|ui| {
            ColorSwatch::new(color)
                .id(id)
                .style(style)
                .show(ui)
                .node(ui)
        });
        let rect = h.arranged(id);
        assert_eq!(
            (rect.unwrap().size.w, rect.unwrap().size.h),
            (side, side),
            "{color:?}"
        );
        assert_eq!(
            h.ui().tree(Layer::Main).shapes_of(node).count(),
            shapes,
            "{color:?}: shapes recorded",
        );
    }
}

/// A chip senses a click and writes nothing — the caller reads it.
#[test]
fn a_chip_reports_its_click() {
    let id = WidgetId::from_hash("chip-click");
    let mut h = Harness::mono(UVec2::new(100, 100));
    let record = |h: &mut Harness| {
        h.frame_value(|ui| {
            ColorSwatch::new(RgbaF32::WHITE)
                .id(id)
                .show(ui)
                .left
                .clicked()
        })
    };
    assert!(!record(&mut h));
    h.click(id);
    assert!(record(&mut h), "the click lands");
    assert!(!record(&mut h), "and is one-shot");
}

/// Tab reaches a chip exactly when the caller leaves it focusable: removing the click sense
/// alone keeps the Tab stop a caller set, and `focusable(false)` takes it away.
#[test]
fn a_chip_is_a_tab_stop_while_the_caller_keeps_it_one() {
    let id = WidgetId::from_hash("chip-tab");
    for (sense, focusable, stop) in [
        (Sense::CLICK, None, true),
        (Sense::NONE, Some(true), true),
        (Sense::NONE, Some(false), false),
        (Sense::CLICK, Some(false), false),
    ] {
        let mut h = Harness::mono(UVec2::new(100, 100));
        let record = |h: &mut Harness| {
            h.frame(|ui| {
                let chip = ColorSwatch::new(RgbaF32::WHITE).id(id).sense(sense);
                match focusable {
                    Some(focusable) => chip.focusable(focusable),
                    None => chip,
                }
                .show(ui);
            });
        };
        record(&mut h);
        h.key(Key::Tab);
        record(&mut h);
        assert_eq!(h.ui().focus() == Some(id), stop, "{sense:?} {focusable:?}");
    }
}

/// A chip frames its color in the chip look of its state, as a button does: its ring at rest,
/// under the pointer and pressed; disabled, the disabled ring, and the veil laid over the color
/// as a second shape.
#[test]
fn a_chip_wears_its_state_and_a_disabled_one_the_veil() {
    use crate::primitives::input::pointer::PointerButton;
    use crate::primitives::paint::border::Border;
    use crate::shape::paint::lowered_border::LoweredBorder;

    let id = WidgetId::from_hash("chip-states");
    let mut style = Theme::stock(ColorScheme::Dark).color_picker;
    style.chip.defaults.transition = AnimationSpec::SNAP;
    let looks = style.chip.unselected.clone();
    let rings = [
        &looks.normal,
        &looks.hovered,
        &looks.active,
        &looks.disabled,
    ]
    .map(|look| look.chrome.border);
    for (i, a) in rings.iter().enumerate() {
        for b in &rings[i + 1..] {
            assert_ne!(a, b, "premise: each state rings apart");
        }
    }
    let mut h = Harness::mono(UVec2::new(100, 100));
    let frame = |h: &mut Harness, disabled: bool| {
        let node = h.frame_value(|ui| {
            ColorSwatch::new(RgbaF32::WHITE)
                .id(id)
                .style(&style)
                .disabled(disabled)
                .show(ui)
                .node(ui)
        });
        let tree = h.ui().tree(Layer::Main);
        let ring = tree.chrome(node).expect("the chip wears a ring").border;
        (ring, tree.shapes_of(node).count())
    };
    let want = |ring: Border, shapes: usize| (LoweredBorder::lower(ring), shapes);
    assert_eq!(frame(&mut h, false), want(rings[0], 1), "at rest");
    h.hover(id);
    assert_eq!(frame(&mut h, false), want(rings[1], 1), "hovered");
    h.press(id, PointerButton::Left);
    assert_eq!(frame(&mut h, false), want(rings[2], 1), "pressed");
    h.release(PointerButton::Left);
    frame(&mut h, false);
    assert_eq!(frame(&mut h, true), want(rings[3], 2), "disabled, veiled");
}
