use crate::host::harness::Harness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::shape::geometry::stroke_geometry::StrokeGeometry;
use crate::theme::Theme;
use crate::widget_core::configure::Configure;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// The tick is themed: `ToggleTheme::check_points` holds it in unit space and `check_polyline` scales it by the box's side, `box_size` held to a pixel, so the drawn polyline tracks both and stays inside the box.
#[test]
fn checkmark_polyline_is_themed_and_scales_with_box_size() {
    use crate::scene::layer::Layer;
    use crate::shape::record::ShapeRecord;
    use crate::theme::color_scheme::ColorScheme;
    use crate::theme::toggle::ToggleTheme;

    fn drawn(theme: ToggleTheme) -> Vec<Vec2> {
        let mut h = Harness::mono(UVec2::new(200, 100));
        h.ui_mut().theme_mut().checkbox = theme;
        let mut v = true;
        h.frame(|ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Checkbox::new(&mut v)
                    .id(WidgetId::from_hash("themed-cb"))
                    .show(ui);
            });
        });
        let tree = h.ui().tree(Layer::Main);
        let span = tree
            .shapes
            .records
            .iter()
            .find_map(|s| match s {
                ShapeRecord::Stroke {
                    geometry: StrokeGeometry::Polyline(polyline),
                    ..
                } => Some(polyline.points),
                _ => None,
            })
            .expect("a checked Checkbox records its tick as a polyline");
        // Points live in the shared record store, addressed by the span.
        let store = h.ui().record_store();
        store.polyline_points[span.range()].to_vec()
    }

    // Stock 16 px box: unit coords land on the hand-tuned pixels (3.5/16 * 16 = 3.5).
    let stock = Theme::stock(ColorScheme::Dark).checkbox;
    assert_eq!(stock.box_size, 16.0);
    assert_eq!(
        drawn(stock.clone()),
        vec![
            Vec2::new(3.5, 8.5),
            Vec2::new(7.0, 12.0),
            Vec2::new(12.5, 4.5),
        ],
    );

    // Double the box, double every coordinate: the tick keeps its proportions.
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

    // A box below a pixel paints at a pixel, and the tick scales by that pixel, inside the box:
    // `3.5 / 16`, `7 / 16` and `12.5 / 16` of one px, not of the quarter px the theme asks.
    let tiny = ToggleTheme {
        box_size: 0.25,
        ..stock.clone()
    };
    assert_eq!(tiny.box_side(), 1.0);
    assert_eq!(
        drawn(tiny),
        vec![
            Vec2::new(3.5 / 16.0, 8.5 / 16.0),
            Vec2::new(7.0 / 16.0, 12.0 / 16.0),
            Vec2::new(12.5 / 16.0, 4.5 / 16.0),
        ],
    );

    // Retheme the shape itself: a straight diagonal, not a tick.
    let diagonal = ToggleTheme {
        check_points: [
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
