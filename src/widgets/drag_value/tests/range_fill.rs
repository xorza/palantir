//! The share of its range a chip shows as a fill under its figures.

use crate::host::harness::Harness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::drag_value::{DragValue, DragValueState};
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// A chip 100 px wide in the stock theme, 3 px of margin around it, its value `value` over
/// `range`, with a range fill.
fn chip(ui: &mut Ui, id: WidgetId, value: &mut f64, range: std::ops::RangeInclusive<f64>) {
    Panel::hstack()
        .id(WidgetId::from_hash("range-fill-row"))
        .show(ui, |ui| {
            DragValue::new(value)
                .range(range)
                .range_fill(true)
                .speed(0.01)
                .size((Sizing::fixed(100.0), Sizing::fixed(30.0)))
                .margin(Spacing::all(3.0))
                .id(id)
                .show(ui);
        });
}

/// 0.25 of [0, 1] fills a quarter of the chip, 25 px of its 100 from the left edge, under the
/// border that draws over its edge, the rest 75 px; the chip, which takes the input and wears
/// the border, keeps the whole box the caller sized and placed, its margin outside it. Opening
/// the editor keeps that box.
#[test]
fn a_range_fill_covers_the_values_share() {
    let id = WidgetId::from_hash("range-fill");
    let mut h = Harness::mono(UVec2::new(300, 60));
    let mut value = 0.25;
    h.frame(|ui| chip(ui, id, &mut value, 0.0..=1.0));
    h.frame(|ui| chip(ui, id, &mut value, 0.0..=1.0));
    let chip_rect = h.rect(id).expect("the chip");
    let fill = h.rect(id.with("fill")).expect("the fill");
    let rest = h.rect(id.with("rest")).expect("the rest");
    assert_eq!((chip_rect.size.w, chip_rect.size.h), (100.0, 30.0));
    assert_eq!(
        chip_rect.min,
        Vec2::new(3.0, 3.0),
        "the margin outside the box"
    );
    assert_eq!(h.rect(id.with("range")), Some(chip_rect), "one box");
    assert_eq!(fill.min, chip_rect.min);
    assert_eq!((fill.size.w, fill.size.h), (25.0, 30.0));
    assert_eq!(rest.size.w, 75.0);
    assert_eq!(rest.max(), chip_rect.max());

    h.ui_mut().set_focus(id);
    h.key(Key::Enter);
    h.frame(|ui| chip(ui, id, &mut value, 0.0..=1.0));
    assert!(
        matches!(
            h.state::<DragValueState>(id),
            Some(DragValueState::Editing { .. })
        ),
        "premise: the editor is open"
    );
    assert_eq!(h.rect(id), Some(chip_rect), "the editor keeps the box");
}

/// The fill follows a scrub as it is held, before any commit: a caller that applies only on the
/// commit re-seeds 0.25 each frame, and 25 px right at 0.01 a pixel reads 0.5, half the chip's
/// 100 px.
#[test]
fn the_fill_follows_a_held_scrub() {
    let id = WidgetId::from_hash("range-fill-scrub");
    let mut h = Harness::mono(UVec2::new(300, 60));
    let canonical = 0.25;
    let frame = |h: &mut Harness| {
        h.frame(|ui| {
            let mut draft = canonical;
            chip(ui, id, &mut draft, 0.0..=1.0);
        });
    };
    frame(&mut h);
    frame(&mut h);
    let center = h.rect(id).expect("the chip").center();
    h.press(center, PointerButton::Left);
    frame(&mut h);
    h.drag_to(center + Vec2::new(25.0, 0.0));
    frame(&mut h);
    frame(&mut h);
    assert_eq!(h.rect(id.with("fill")).map(|r| r.size.w), Some(50.0));
    h.release(PointerButton::Left);
    frame(&mut h);
}

/// A transform the caller gives the chip moves its box and its fill with it: translated 10 px
/// right, the fill still starts at the chip's left edge and spans a quarter of it.
#[test]
fn a_transform_moves_the_fill_with_the_chip() {
    let id = WidgetId::from_hash("range-fill-moved");
    let mut h = Harness::mono(UVec2::new(300, 60));
    let mut value = 0.25;
    let mut show = |h: &mut Harness| {
        h.frame(|ui| {
            DragValue::new(&mut value)
                .range(0.0..=1.0)
                .range_fill(true)
                .size((Sizing::fixed(100.0), Sizing::fixed(30.0)))
                .transform(TranslateScale::new(Vec2::new(10.0, 0.0), 1.0))
                .id(id)
                .show(ui);
        });
    };
    show(&mut h);
    show(&mut h);
    let chip = h.rect(id).expect("the chip");
    let fill = h.rect(id.with("fill")).expect("the fill");
    assert_eq!(chip.min.x, 10.0);
    assert_eq!(fill.min, chip.min);
    assert_eq!(fill.size.w, 25.0);
}

/// No fill shows while the setter is off, while an end of the range is infinite, or while the
/// editor is open; the chip is then one node, as a plain drag value is.
#[test]
fn no_fill_shows_unbounded_off_or_typing() {
    let id = WidgetId::from_hash("range-fill-off");
    let mut h = Harness::mono(UVec2::new(300, 60));
    let mut value = 0.25;
    let mut show = |h: &mut Harness, fill: bool, max: f64| {
        h.frame(|ui| {
            DragValue::new(&mut value)
                .range(0.0..=max)
                .range_fill(fill)
                .id(id)
                .show(ui);
        });
        h.frame(|ui| {
            DragValue::new(&mut value)
                .range(0.0..=max)
                .range_fill(fill)
                .id(id)
                .show(ui);
        });
        h.rect(id.with("fill")).is_some()
    };
    assert!(show(&mut h, true, 1.0), "premise: a bounded fill shows");
    assert!(!show(&mut h, false, 1.0), "off");
    assert!(!show(&mut h, true, f64::INFINITY), "no ceiling");
    assert!(show(&mut h, true, 1.0));
    h.ui_mut().set_focus(id);
    h.key(Key::Enter);
    assert!(!show(&mut h, true, 1.0), "typing");
}

/// The fill wears the theme's range fill on a chip that takes input, and its disabled one on a
/// disabled chip.
#[test]
fn a_disabled_chip_wears_the_disabled_fill() {
    let id = WidgetId::from_hash("range-fill-disabled");
    for disabled in [false, true] {
        let mut h = Harness::mono(UVec2::new(300, 60));
        let mut value = 0.5;
        h.frame(|ui| {
            DragValue::new(&mut value)
                .range(0.0..=1.0)
                .range_fill(true)
                .disabled(disabled)
                .id(id)
                .show(ui);
        });
        let theme = h.ui().theme().drag_value.clone();
        assert_ne!(theme.range_fill, theme.range_fill_disabled, "premise");
        let want = if disabled {
            theme.range_fill_disabled
        } else {
            theme.range_fill
        }
        .to_f16();
        let node = h.node_of(id.with("fill")).expect("the fill").node;
        let fill = h
            .ui()
            .tree(Layer::Main)
            .chrome(node)
            .expect("the fill paints chrome")
            .fill;
        assert!(
            matches!(fill, LoweredBrush::Solid(got) if got == want),
            "disabled {disabled}: {fill:?}, want {want:?}"
        );
    }
}
