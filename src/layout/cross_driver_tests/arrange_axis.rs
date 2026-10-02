use crate::Ui;
use crate::layout::axis::Axis;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::sizing::{SizeSpec, Sizing};
use crate::layout::types::track::Track;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
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
    /// A fixed child of this extent on the axis, for a hugging case to
    /// hug; zero records none.
    content: f32,
}

fn axis_sizes(axis: Axis, sizing: Sizing) -> SizeSpec {
    match axis {
        Axis::X => SizeSpec::new(sizing, Sizing::fixed(10.0)),
        Axis::Y => SizeSpec::new(Sizing::fixed(10.0), sizing),
    }
}

fn add_child(ui: &mut Ui, id: WidgetId, case: ArrangeCase) {
    Panel::zstack()
        .id(id)
        .size(axis_sizes(case.axis, case.sizing))
        .min_size(case.axis.compose_size(case.min, 0.0))
        .max_size(case.axis.compose_size(case.max, f32::INFINITY))
        .margin(case.margin)
        .align(case.align)
        .show(ui, |ui| {
            if case.content > 0.0 {
                Block::new()
                    .auto_id()
                    .size(axis_sizes(case.axis, Sizing::fixed(case.content)))
                    .show(ui);
            }
        });
}

fn arrange_with(driver: Driver, case: ArrangeCase) -> Rect {
    let child = WidgetId::from_hash("arrange-axis-child");
    let parent_size = case.axis.compose_size(case.slot, 100.0);
    let surface = UVec2::new(parent_size.w as u32, parent_size.h as u32);
    let mut h = UiHarness::new(surface);
    h.frame(|ui| match driver {
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
                    Block::new()
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
    h.arranged(child)
}

/// One sizing resolves to one extent under every driver, on either axis,
/// with or without a margin — and the margin moves the child off the
/// slot's start by itself, since the default alignment starts there.
///
/// Fill floors at its measured minimum when the slot is too small, and
/// stops at its maximum when the slot is larger; a fixed size holds under
/// stretch alignment, loses to a larger minimum, and a minimum equal to
/// the maximum pins the extent outright. A hugging node stops at its
/// maximum below its content.
#[test]
fn sizing_resolves_alike_under_every_driver() {
    #[derive(Debug)]
    struct Row {
        label: &'static str,
        sizing: Sizing,
        slot: f32,
        min: f32,
        max: f32,
        content: f32,
        extent: f32,
    }
    let row = |label, sizing, slot, min, max, content, extent| Row {
        label,
        sizing,
        slot,
        min,
        max,
        content,
        extent,
    };
    let inf = f32::INFINITY;
    let rows = [
        row("fill floors", Sizing::FILL, 50.0, 80.0, inf, 0.0, 80.0),
        row("fill caps", Sizing::FILL, 200.0, 0.0, 80.0, 0.0, 80.0),
        row(
            "fixed holds",
            Sizing::fixed(20.0),
            100.0,
            0.0,
            inf,
            0.0,
            20.0,
        ),
        row(
            "fixed under min",
            Sizing::fixed(20.0),
            100.0,
            30.0,
            inf,
            0.0,
            30.0,
        ),
        row("min equals max", Sizing::FILL, 200.0, 40.0, 40.0, 0.0, 40.0),
        row("hug under max", Sizing::HUG, 100.0, 0.0, 40.0, 60.0, 40.0),
    ];
    for Row {
        label,
        sizing,
        slot,
        min,
        max,
        content,
        extent,
    } in rows
    {
        for axis in [Axis::X, Axis::Y] {
            for driver in DRIVERS {
                for margin in [0.0, 10.0] {
                    let case = ArrangeCase {
                        axis,
                        slot,
                        sizing,
                        min,
                        max,
                        margin,
                        align: Align::default(),
                        content,
                    };
                    let rect = arrange_with(driver, case);
                    let at = format!("{label}: {axis:?} {driver:?} margin {margin}");
                    assert_eq!(axis.main(rect.size), extent, "{at}");
                    assert_eq!(axis.main_v(rect.min), margin, "{at}: start");
                }
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
                    content: 0.0,
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

/// A container with no children hugs nothing: every driver arranges an
/// empty hugging panel to its padding alone, 5 on each side.
#[test]
fn an_empty_driver_hugs_its_padding() {
    let id = WidgetId::from_hash("empty-driver");
    for driver in [
        Driver::Canvas,
        Driver::Stack,
        Driver::WrapStack,
        Driver::ZStack,
        Driver::Grid,
    ] {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| {
            let hug = (Sizing::HUG, Sizing::HUG);
            match driver {
                Driver::Root => unreachable!("the root is not a container"),
                Driver::Canvas => Panel::canvas()
                    .id(id)
                    .size(hug)
                    .padding(5.0)
                    .show(ui, |_| {}),
                Driver::Stack => Panel::hstack()
                    .id(id)
                    .size(hug)
                    .padding(5.0)
                    .show(ui, |_| {}),
                Driver::WrapStack => Panel::wrap_hstack()
                    .id(id)
                    .size(hug)
                    .padding(5.0)
                    .show(ui, |_| {}),
                Driver::ZStack => Panel::zstack()
                    .id(id)
                    .size(hug)
                    .padding(5.0)
                    .show(ui, |_| {}),
                Driver::Grid => {
                    Grid::new()
                        .id(id)
                        .cols([Track::HUG])
                        .rows([Track::HUG])
                        .size(hug)
                        .padding(5.0)
                        .show(ui, |_| {});
                    return;
                }
            };
        });
        assert_eq!(
            h.arranged(id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            "{driver:?}"
        );
    }
}
