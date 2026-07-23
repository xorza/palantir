use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::widgets::combo_box::{ComboBox, ComboState};
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 300);

#[test]
fn dropdown_aligns_to_the_full_trigger_rect_when_flipped_above() {
    let mut ui = Ui::for_test();
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
    ui.run_at(SURFACE, |ui| build(ui, &mut selected));
    ui.state_mut::<ComboState>(id).open = true;

    let mut passes = 0;
    ui.run_at(SURFACE, |ui| {
        passes += 1;
        build(ui, &mut selected);
    });
    assert_eq!(passes, 1, "dropdown placement must converge in one pass");

    let trigger = ui.response_for(id).rect.expect("combo trigger arranged");
    let list = ui
        .response_for(id.with("list"))
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
