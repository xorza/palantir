use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

fn run(value: &mut bool, h: &mut UiHarness) {
    let mut v = *value;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut v)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .show(ui);
        });
    });
    *value = v;
}

#[test]
fn clicking_toggles_value() {
    let surface = UVec2::new(300, 100);
    let mut h = UiHarness::new(surface);
    let mut v = false;

    // Frame 1: lay out so the row has a rect.
    let mut rec = v;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .show(ui);
        });
    });
    v = rec;
    assert!(!v, "starts unchecked");

    // Click on the box area.
    h.click_at(Vec2::new(8.0, 8.0));
    run(&mut v, &mut h);
    assert!(v, "single click toggles on");

    h.click_at(Vec2::new(8.0, 8.0));
    run(&mut v, &mut h);
    assert!(!v, "second click toggles off");
}

#[test]
fn disabled_checkbox_does_not_toggle() {
    let surface = UVec2::new(300, 100);
    let mut h = UiHarness::new(surface);
    let mut v = false;

    let mut rec = v;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .disabled(true)
                .show(ui);
        });
    });
    v = rec;

    h.click_at(Vec2::new(8.0, 8.0));
    let mut rec = v;
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Checkbox::new(&mut rec)
                .id(WidgetId::from_hash("cb"))
                .label("label")
                .disabled(true)
                .show(ui);
        });
    });
    v = rec;
    assert!(!v, "disabled checkbox swallows click");
}

/// The tick is themed, not baked in: `ToggleTheme::check_pts` holds it
/// in unit space and `check_polyline` scales it by `box_size`, so the
/// drawn polyline tracks both.
///
/// Unit space is what removes the old `box_size / 16.0` reference — the
/// shape no longer carries a size of its own that could drift from
/// `box_size`.
#[test]
fn checkmark_polyline_is_themed_and_scales_with_box_size() {
    use crate::scene::layer::Layer;
    use crate::scene::shapes::record::ShapeRecord;
    use crate::widgets::theme::toggle::ToggleTheme;

    fn drawn(theme: ToggleTheme) -> Vec<Vec2> {
        let mut h = UiHarness::new(UVec2::new(200, 100));
        h.ui.theme_mut().checkbox = theme;
        let mut v = true;
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Checkbox::new(&mut v)
                    .id(WidgetId::from_hash("themed-cb"))
                    .show(ui);
            });
        });
        let tree = h.ui.tree(Layer::Main);
        let span = tree
            .shapes
            .records
            .iter()
            .find_map(|s| match s {
                ShapeRecord::Polyline { points, .. } => Some(*points),
                _ => None,
            })
            .expect("a checked Checkbox records its tick as a polyline");
        // Points live in the shared record store, addressed by the span.
        let store = h.ui.payloads();
        store.polyline_points[span.range()].to_vec()
    }

    // Stock 16 px box: unit coords land back on the hand-tuned pixels
    // they were derived from (3.5/16 * 16 = 3.5, and so on).
    let stock = ToggleTheme::checkbox(&crate::widgets::theme::palette::Palette::DEFAULT);
    assert_eq!(stock.box_size, 16.0);
    assert_eq!(
        drawn(stock.clone()),
        vec![
            Vec2::new(3.5, 8.5),
            Vec2::new(7.0, 12.0),
            Vec2::new(12.5, 4.5),
        ],
    );

    // Double the box, double every coordinate — the tick keeps its
    // proportions instead of sitting in a corner.
    let big = ToggleTheme {
        box_size: 32.0,
        ..stock.clone()
    };
    assert_eq!(
        drawn(big),
        vec![
            Vec2::new(7.0, 17.0),
            Vec2::new(14.0, 24.0),
            Vec2::new(25.0, 9.0),
        ],
    );

    // Retheme the shape itself: a straight diagonal, not a tick.
    let diagonal = ToggleTheme {
        check_pts: [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.5, 0.5),
            Vec2::new(1.0, 1.0),
        ],
        ..stock
    };
    assert_eq!(
        drawn(diagonal),
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(8.0, 8.0),
            Vec2::new(16.0, 16.0),
        ],
    );
}
