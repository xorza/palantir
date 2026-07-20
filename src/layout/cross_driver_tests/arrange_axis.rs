use crate::Ui;
use crate::layout::axis::Axis;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::{Sizes, Sizing};
use crate::layout::types::track::Track;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::Configure;
use crate::widgets::frame::Frame;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use glam::UVec2;

#[derive(Clone, Copy, Debug)]
enum Driver {
    Root,
    Canvas,
    Stack,
    WrapStack,
    ZStack,
    Grid,
}

const DRIVERS: [Driver; 6] = [
    Driver::Root,
    Driver::Canvas,
    Driver::Stack,
    Driver::WrapStack,
    Driver::ZStack,
    Driver::Grid,
];

const ALIGNED_DRIVERS: [Driver; 4] = [
    Driver::Stack,
    Driver::WrapStack,
    Driver::ZStack,
    Driver::Grid,
];

#[derive(Clone, Copy, Debug)]
struct ArrangeCase {
    axis: Axis,
    slot: f32,
    sizing: Sizing,
    min: f32,
    max: f32,
    margin: f32,
    align: Align,
}

fn axis_sizes(axis: Axis, sizing: Sizing) -> Sizes {
    match axis {
        Axis::X => Sizes::new(sizing, Sizing::fixed(10.0)),
        Axis::Y => Sizes::new(Sizing::fixed(10.0), sizing),
    }
}

fn add_child(ui: &mut Ui, id: WidgetId, case: ArrangeCase) {
    Frame::new()
        .id(id)
        .size(axis_sizes(case.axis, case.sizing))
        .min_size(case.axis.compose_size(case.min, 0.0))
        .max_size(case.axis.compose_size(case.max, f32::INFINITY))
        .margin(case.margin)
        .align(case.align)
        .show(ui);
}

fn arrange_with(driver: Driver, case: ArrangeCase) -> Rect {
    let mut ui = Ui::for_test();
    let child = WidgetId::from_hash("arrange-axis-child");
    let parent_size = case.axis.compose_size(case.slot, 100.0);
    let surface = UVec2::new(parent_size.w as u32, parent_size.h as u32);
    ui.run_at_without_baseline(surface, |ui| match driver {
        Driver::Root => add_child(ui, child, case),
        Driver::Canvas => {
            Panel::canvas()
                .auto_id()
                .size(parent_size)
                .show(ui, |ui| add_child(ui, child, case));
        }
        Driver::Stack => {
            let panel = match case.axis {
                Axis::X => Panel::vstack(),
                Axis::Y => Panel::hstack(),
            };
            panel
                .auto_id()
                .size(parent_size)
                .child_align(Align::STRETCH)
                .show(ui, |ui| add_child(ui, child, case));
        }
        Driver::WrapStack => {
            let panel = match case.axis {
                Axis::X => Panel::wrap_vstack(),
                Axis::Y => Panel::wrap_hstack(),
            };
            panel
                .auto_id()
                .size(parent_size)
                .child_align(Align::STRETCH)
                .show(ui, |ui| {
                    Frame::new()
                        .auto_id()
                        .size(axis_sizes(case.axis, Sizing::fixed(case.slot)))
                        .show(ui);
                    add_child(ui, child, case);
                });
        }
        Driver::ZStack => {
            Panel::zstack()
                .auto_id()
                .size(parent_size)
                .child_align(Align::STRETCH)
                .show(ui, |ui| add_child(ui, child, case));
        }
        Driver::Grid => {
            Grid::new()
                .auto_id()
                .cols([Track::fixed(parent_size.w)])
                .rows([Track::fixed(parent_size.h)])
                .size(parent_size)
                .show(ui, |ui| add_child(ui, child, case));
        }
    });
    ui.response_for(child).rect.expect("child arranged")
}

#[test]
fn fill_preserves_measured_floor_when_slot_is_undersized() {
    for axis in [Axis::X, Axis::Y] {
        for driver in DRIVERS {
            for margin in [0.0, 10.0] {
                let case = ArrangeCase {
                    axis,
                    slot: 50.0,
                    sizing: Sizing::FILL,
                    min: 80.0,
                    max: f32::INFINITY,
                    margin,
                    align: Align::default(),
                };
                let rect = arrange_with(driver, case);
                assert_eq!(axis.main(rect.size), 80.0, "{axis:?} {driver:?}");
            }
        }
    }
}

#[test]
fn stretch_growth_respects_max_size() {
    for axis in [Axis::X, Axis::Y] {
        for driver in DRIVERS {
            for margin in [0.0, 10.0] {
                let case = ArrangeCase {
                    axis,
                    slot: 200.0,
                    sizing: Sizing::FILL,
                    min: 0.0,
                    max: 80.0,
                    margin,
                    align: Align::default(),
                };
                let rect = arrange_with(driver, case);
                assert_eq!(axis.main(rect.size), 80.0, "{axis:?} {driver:?}");
            }
        }
    }
}

#[test]
fn fixed_remains_exact_under_stretch_alignment() {
    for axis in [Axis::X, Axis::Y] {
        for driver in DRIVERS {
            for margin in [0.0, 10.0] {
                let case = ArrangeCase {
                    axis,
                    slot: 100.0,
                    sizing: Sizing::fixed(20.0),
                    min: 0.0,
                    max: f32::INFINITY,
                    margin,
                    align: Align::default(),
                };
                let rect = arrange_with(driver, case);
                assert_eq!(axis.main(rect.size), 20.0, "{axis:?} {driver:?}");
            }
        }
    }
}

#[test]
fn max_capped_fill_uses_resolved_alignment() {
    for axis in [Axis::X, Axis::Y] {
        let cases = match axis {
            Axis::X => [
                (Align::h(HAlign::Center), 60.0),
                (Align::h(HAlign::Right), 120.0),
            ],
            Axis::Y => [
                (Align::v(VAlign::Center), 60.0),
                (Align::v(VAlign::Bottom), 120.0),
            ],
        };
        for driver in ALIGNED_DRIVERS {
            for (align, expected_offset) in cases {
                let case = ArrangeCase {
                    axis,
                    slot: 200.0,
                    sizing: Sizing::FILL,
                    min: 0.0,
                    max: 80.0,
                    margin: 0.0,
                    align,
                };
                let rect = arrange_with(driver, case);
                assert_eq!(axis.main(rect.size), 80.0, "{axis:?} {driver:?}");
                assert_eq!(
                    axis.main_v(rect.min),
                    expected_offset,
                    "{axis:?} {driver:?} {align:?}",
                );
            }
        }
    }
}
