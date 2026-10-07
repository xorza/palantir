#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
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
    /// A fixed child of this extent on the axis for a hugging case to hug; zero records none.
    content: f32,
}

fn axis_sizes(axis: Axis, sizing: Sizing) -> SizeSpec {
    axis.compose_sizing(sizing, Sizing::fixed(10.0))
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
            let panel = Panel::stack(case.axis.other());
            panel
                .auto_id()
                .size(parent_size)
                .child_align(Align::STRETCH)
                .show(ui, |ui| add_child(ui, child, case));
        }
        Driver::WrapStack => {
            let panel = Panel::wrap_stack_on(case.axis.other());
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

/// One sizing resolves to one extent under every driver, either axis, with or without margin (which alone moves the child off the slot start). Fill floors at its measured minimum in a small slot and stops at its maximum in a larger one; fixed holds under stretch, loses to a larger minimum, and min == max pins it. A hugging node stops at its maximum below its content and floors at its minimum above content and slot (a stretching driver grows a hugging child to the slot).
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
        row("hug over min", Sizing::HUG, 40.0, 50.0, inf, 30.0, 50.0),
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

/// A childless hugging panel arranges to its padding alone, 5 on each side, under every driver.
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
            let panel = match driver {
                Driver::Root => unreachable!("the root is not a container"),
                Driver::Canvas => Panel::canvas(),
                Driver::Stack => Panel::hstack(),
                Driver::WrapStack => Panel::wrap_hstack(),
                Driver::ZStack => Panel::zstack(),
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
            panel.id(id).size(hug).padding(5.0).show(ui, |_| {});
        });
        assert_eq!(
            h.arranged(id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            "{driver:?}"
        );
    }
}

/// A collapsed child takes no room under any driver: no hugging growth, gap, or sibling shift, zero size. Children: 20×20 `a`, collapsed 50×50 `gone`, 30×20 `b`, gap 10. Stack and wrap put `b` at 20 + 10 = 30 and hug 30 + 30 = 60. Canvas places `b` by hand and `gone` at (100, 100). Grid puts `gone` in `a`'s cell and `b` in the next column. Zstack overlaps `a` and `b` at the origin and hugs the wider, 30.
#[test]
fn a_collapsed_child_takes_no_room() {
    let panel_id = WidgetId::from_hash("collapsed-driver");
    let [a_id, gone_id, b_id] = ["a", "gone", "b"].map(WidgetId::from_hash);
    for driver in [
        Driver::Canvas,
        Driver::Stack,
        Driver::WrapStack,
        Driver::ZStack,
        Driver::Grid,
    ] {
        let child = |ui: &mut Ui, id: WidgetId, size: (f32, f32), at: (f32, f32), col: u16| {
            let mut block = Block::new().id(id).size(size);
            match driver {
                Driver::Canvas => block = block.position(at),
                Driver::Grid => block = block.grid_cell((0, col)),
                _ => {}
            }
            if id == gone_id {
                block = block.collapsed();
            }
            block.show(ui);
        };
        let children = |ui: &mut Ui| {
            child(ui, a_id, (20.0, 20.0), (0.0, 0.0), 0);
            child(ui, gone_id, (50.0, 50.0), (100.0, 100.0), 0);
            child(ui, b_id, (30.0, 20.0), (30.0, 0.0), 1);
        };
        let mut h = UiHarness::new(UVec2::new(400, 400));
        h.frame(|ui| {
            let hug = (Sizing::HUG, Sizing::HUG);
            let panel = match driver {
                Driver::Root => unreachable!("the root is not a container"),
                Driver::Canvas => Panel::canvas(),
                Driver::Stack => Panel::hstack(),
                Driver::WrapStack => Panel::wrap_hstack(),
                Driver::ZStack => Panel::zstack(),
                Driver::Grid => {
                    Grid::new()
                        .id(panel_id)
                        .cols([Track::HUG, Track::HUG])
                        .rows([Track::HUG])
                        .size(hug)
                        .gap(10.0)
                        .show(ui, children);
                    return;
                }
            };
            panel.id(panel_id).size(hug).gap(10.0).show(ui, children);
        });
        let (b_x, panel_w) = match driver {
            Driver::ZStack => (0.0, 30.0),
            _ => (30.0, 60.0),
        };
        assert_eq!(
            h.arranged(panel_id),
            Rect::new(0.0, 0.0, panel_w, 20.0),
            "{driver:?}: the panel"
        );
        assert_eq!(
            h.arranged(a_id),
            Rect::new(0.0, 0.0, 20.0, 20.0),
            "{driver:?}: a"
        );
        assert_eq!(
            h.arranged(b_id),
            Rect::new(b_x, 0.0, 30.0, 20.0),
            "{driver:?}: b"
        );
        assert_eq!(
            h.arranged(gone_id).size,
            Size::ZERO,
            "{driver:?}: the collapsed child"
        );
    }
}
