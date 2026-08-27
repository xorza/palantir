use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::combo_box::{ComboBox, ComboState};
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 300);

/// A selection the option list doesn't contain has no rendering: the
/// trigger shows the current choice and there is no placeholder. Falling
/// back to a blank label made a broken caller model look like an
/// ordinary empty control, so it panics instead.
///
/// An empty list is the same failure — every index is out of range —
/// which is why it is the second case rather than a carve-out.
#[test]
#[should_panic(expected = "out of range for 1 option(s)")]
fn an_out_of_range_selection_panics() {
    let mut h = UiHarness::new(SURFACE);
    let mut selected = 3;
    h.frame(|ui| {
        ComboBox::new(&mut selected, &["One"])
            .id(WidgetId::from_hash("combo"))
            .show(ui);
    });
}

#[test]
#[should_panic(expected = "out of range for 0 option(s)")]
fn an_empty_option_list_panics() {
    let mut h = UiHarness::new(SURFACE);
    let mut selected = 0;
    h.frame(|ui| {
        ComboBox::new(&mut selected, &[] as &[&str])
            .id(WidgetId::from_hash("combo"))
            .show(ui);
    });
}

/// `labeled` reads the row's projected field, not the row: a dropdown over
/// records measures exactly as one over the string it projects, and not as
/// one over the row's other string.
///
/// Asserted through the trigger label's width because that is the only
/// thing the option text can reach from outside — and it is enough, since
/// the two candidate fields differ in length.
#[test]
fn a_labeled_dropdown_reads_the_projected_field() {
    /// A row that is not itself text: no `AsRef<str>` impl could pick
    /// between these two, which is the case `labeled` exists for.
    #[derive(Debug)]
    struct Row {
        name: &'static str,
        display: &'static str,
    }
    let rows = [Row {
        name: "a",
        display: "Elderberry",
    }];

    let (projected, other, literal) = (
        WidgetId::from_hash("projected"),
        WidgetId::from_hash("other"),
        WidgetId::from_hash("literal"),
    );
    let (mut a, mut b, mut c) = (0, 0, 0);
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ComboBox::labeled(&mut a, &rows, |r| r.display)
                    .id(projected)
                    .show(ui);
                ComboBox::labeled(&mut b, &rows, |r| r.name)
                    .id(other)
                    .show(ui);
                ComboBox::new(&mut c, &["Elderberry"]).id(literal).show(ui);
            });
    });

    let width = |id: WidgetId| {
        h.rect(id.with("label"))
            .expect("trigger label arranged")
            .size
            .w
    };
    assert_eq!(
        width(projected),
        width(literal),
        "the trigger measured the row's `display`, so it read that field",
    );
    assert_ne!(
        width(projected),
        width(other),
        "and the projection is what chose it — `name` renders differently",
    );
}

#[test]
fn dropdown_aligns_to_the_full_trigger_rect_when_flipped_above() {
    let mut h = UiHarness::new(SURFACE);
    let id = WidgetId::from_hash("combo");
    let options = ["One", "Two", "Three"];
    let mut selected = 0;
    let build = |ui: &mut Ui, selected: &mut usize| {
        Panel::canvas()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ComboBox::new(selected, &options)
                    .id(id)
                    .position(Vec2::new(120.0, 250.0))
                    .size((Sizing::fixed(140.0), Sizing::fixed(30.0)))
                    .show(ui);
            });
    };
    h.frame(|ui| build(ui, &mut selected));
    h.ui.state_mut::<ComboState>(id).open = true;

    let mut passes = 0;
    h.frame(|ui| {
        passes += 1;
        build(ui, &mut selected);
    });
    assert_eq!(passes, 1, "dropdown placement must converge in one pass");

    let trigger = h.ui.response_for(id).rect.expect("combo trigger arranged");
    let list =
        h.ui.response_for(id.with("list"))
            .rect
            .expect("combo list arranged");
    assert_eq!(list.min.x, trigger.min.x, "list starts at trigger left");
    assert_eq!(
        list.max().y,
        trigger.min.y,
        "above fallback ends at the trigger's top edge",
    );
    assert!(
        list.size.w >= trigger.size.w,
        "list width {} must cover trigger width {}",
        list.size.w,
        trigger.size.w,
    );
}

/// The trigger's shape comes from `Theme::combo_box`, not from
/// constants: the chevron node is sized from `arrow_size`, and the
/// gutter between label and arrow from `row_gap`.
///
/// Hug-sized so `Justify::SpaceBetween` has no free space to
/// distribute — the rendered gap is then exactly `row_gap`, which a
/// fixed-width trigger would hide behind the justification slack.
#[test]
fn trigger_geometry_follows_the_combo_box_theme() {
    let options = ["One"];
    let id = WidgetId::from_hash("geom-combo");

    let measure = |arrow: Vec2, row_gap: f32| -> (Vec2, f32) {
        let mut h = UiHarness::new(SURFACE);
        h.ui.theme_mut().combo_box.arrow_size = arrow;
        h.ui.theme_mut().combo_box.row_gap = row_gap;
        let mut selected = 0;
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    ComboBox::new(&mut selected, &options)
                        .id(id)
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui);
                });
        });
        let label =
            h.ui.response_for(id.with("label"))
                .rect
                .expect("label arranged");
        let arrow_rect =
            h.ui.response_for(id.with("arrow"))
                .rect
                .expect("arrow arranged");
        (
            Vec2::new(arrow_rect.size.w, arrow_rect.size.h),
            arrow_rect.min.x - label.max().x,
        )
    };

    let (size_a, gap_a) = measure(Vec2::new(10.0, 6.0), 12.0);
    assert_eq!(size_a, Vec2::new(10.0, 6.0), "arrow node takes arrow_size");
    assert!(
        (gap_a - 12.0).abs() < 1e-4,
        "gutter is row_gap, got {gap_a}",
    );

    // Both knobs move the layout — neither is baked in.
    let (size_b, gap_b) = measure(Vec2::new(20.0, 14.0), 30.0);
    assert_eq!(size_b, Vec2::new(20.0, 14.0));
    assert!(
        (gap_b - 30.0).abs() < 1e-4,
        "gutter is row_gap, got {gap_b}"
    );
    assert_ne!(size_a, size_b);
    assert_ne!(gap_a, gap_b);
}
