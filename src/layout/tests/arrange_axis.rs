use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

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

/// The drivers that hold children, every one but the root.
const CONTAINERS: [Driver; 5] = [
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

fn add_child(rec: &mut NodeRecorder<'_>, case: ArrangeCase) {
    let child = Widget::zstack()
        .size(axis_sizes(case.axis, case.sizing))
        .min_size(case.axis.compose_size(case.min, 0.0))
        .max_size(case.axis.compose_size(case.max, f32::INFINITY))
        .margin(case.margin)
        .align(case.align);
    rec.node("child", child, |rec| {
        if case.content > 0.0 {
            let content = Widget::leaf().size(axis_sizes(case.axis, Sizing::fixed(case.content)));
            rec.leaf("content", content);
        }
    });
}

fn arrange_with(driver: Driver, case: ArrangeCase) -> Rect {
    let parent_size = case.axis.compose_size(case.slot, 100.0);
    let mut rig = LayoutRig::new();
    rig.run(parent_size, |rec| match driver {
        Driver::Root => add_child(rec, case),
        Driver::Canvas => {
            rec.node("parent", Widget::canvas().size(parent_size), |rec| {
                add_child(rec, case);
            });
        }
        Driver::Stack => {
            let parent = Widget::stack(case.axis.other())
                .size(parent_size)
                .child_align(Align::STRETCH);
            rec.node("parent", parent, |rec| add_child(rec, case));
        }
        Driver::WrapStack => {
            let parent = Widget::wrap_stack(case.axis.other())
                .size(parent_size)
                .child_align(Align::STRETCH);
            rec.node("parent", parent, |rec| {
                let line = Widget::leaf().size(axis_sizes(case.axis, Sizing::fixed(case.slot)));
                rec.leaf("line", line);
                add_child(rec, case);
            });
        }
        Driver::ZStack => {
            let parent = Widget::zstack()
                .size(parent_size)
                .child_align(Align::STRETCH);
            rec.node("parent", parent, |rec| add_child(rec, case));
        }
        Driver::Grid => {
            let tracks = GridTracks {
                rows: &[Track::fixed(parent_size.h)],
                cols: &[Track::fixed(parent_size.w)],
            };
            rec.grid("parent", Widget::grid().size(parent_size), tracks, |rec| {
                add_child(rec, case);
            });
        }
    });
    rig.rect("child")
}

/// A hugging container of `driver`, with the setters `configure` adds, around `body`, under
/// the id `id`; a grid takes `cols` Hug columns and one Hug row.
fn hug_driver(
    rec: &mut NodeRecorder<'_>,
    driver: Driver,
    cols: &[Track],
    configure: impl FnOnce(Widget) -> Widget,
    body: impl FnOnce(&mut NodeRecorder<'_>),
) {
    let id = "panel";
    let widget = match driver {
        Driver::Root => unreachable!("the root is not a container"),
        Driver::Canvas => Widget::canvas(),
        Driver::Stack => Widget::hstack(),
        Driver::WrapStack => Widget::wrap_hstack(),
        Driver::ZStack => Widget::zstack(),
        Driver::Grid => {
            let tracks = GridTracks {
                rows: &[Track::HUG],
                cols,
            };
            rec.grid(id, configure(Widget::grid()), tracks, body);
            return;
        }
    };
    rec.node(id, configure(widget), body);
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
    for driver in CONTAINERS {
        let mut rig = LayoutRig::new();
        rig.run(Size::new(200.0, 200.0), |rec| {
            hug_driver(rec, driver, &[Track::HUG], |w| w.padding(5.0), |_| {});
        });
        assert_eq!(
            rig.rect("panel"),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            "{driver:?}"
        );
    }
}

/// A collapsed child takes no room under any driver: no hugging growth, gap, or sibling shift, zero size. Children: 20×20 `a`, collapsed 50×50 `gone`, 30×20 `b`, gap 10. Stack and wrap put `b` at 20 + 10 = 30 and hug 30 + 30 = 60. Canvas places `b` by hand and `gone` at (100, 100). Grid puts `gone` in `a`'s cell and `b` in the next column. Zstack overlaps `a` and `b` at the origin and hugs the wider, 30.
#[test]
fn a_collapsed_child_takes_no_room() {
    for driver in CONTAINERS {
        let child =
            |rec: &mut NodeRecorder<'_>, id: &str, size: (f32, f32), at: (f32, f32), col: u16| {
                let mut leaf = Widget::leaf().size(size);
                match driver {
                    Driver::Canvas => leaf = leaf.at(at),
                    Driver::Grid => leaf = leaf.grid_cell((0, col)),
                    _ => {}
                }
                if id == "gone" {
                    leaf = leaf.collapsed();
                }
                rec.leaf(id, leaf);
            };
        let mut rig = LayoutRig::new();
        rig.run(Size::new(400.0, 400.0), |rec| {
            hug_driver(
                rec,
                driver,
                &[Track::HUG, Track::HUG],
                |w| w.gap(10.0),
                |rec| {
                    child(rec, "a", (20.0, 20.0), (0.0, 0.0), 0);
                    child(rec, "gone", (50.0, 50.0), (100.0, 100.0), 0);
                    child(rec, "b", (30.0, 20.0), (30.0, 0.0), 1);
                },
            );
        });
        let (b_x, panel_w) = match driver {
            Driver::ZStack => (0.0, 30.0),
            _ => (30.0, 60.0),
        };
        assert_eq!(
            rig.rect("panel"),
            Rect::new(0.0, 0.0, panel_w, 20.0),
            "{driver:?}: the panel"
        );
        assert_eq!(
            rig.rect("a"),
            Rect::new(0.0, 0.0, 20.0, 20.0),
            "{driver:?}: a"
        );
        assert_eq!(
            rig.rect("b"),
            Rect::new(b_x, 0.0, 30.0, 20.0),
            "{driver:?}: b"
        );
        assert_eq!(
            rig.rect("gone").size,
            Size::ZERO,
            "{driver:?}: the collapsed child"
        );
    }
}

/// A hidden child keeps its slot: in a gap-10 row of three 40-wide blocks, the hidden middle one
/// sits at 40 + 10 = 50 and `b` past it and both gaps, at 50 + 40 + 10 = 100.
#[test]
fn a_hidden_child_keeps_its_slot() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 100.0), |rec| {
        rec.node("row", Widget::hstack().gap(10.0), |rec| {
            rec.leaf("a", Widget::leaf().size(40.0));
            rec.leaf("hid", Widget::leaf().size(40.0).hidden());
            rec.leaf("b", Widget::leaf().size(40.0));
        });
    });
    assert_eq!(rig.rect("hid"), Rect::new(50.0, 0.0, 40.0, 40.0));
    assert_eq!(rig.rect("b"), Rect::new(100.0, 0.0, 40.0, 40.0));
}

/// A collapsed fill child drops its weight: beside it, two weight-1 fills split a 400-wide row
/// 200 / 200, where its weight 3 would have left them 80 each.
#[test]
fn a_collapsed_child_drops_its_fill_weight() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(400.0, 100.0), |rec| {
        rec.node(
            "row",
            Widget::hstack().size((Sizing::FILL, Sizing::HUG)),
            |rec| {
                let fill = |weight| Widget::leaf().size((Sizing::fill(weight), Sizing::HUG));
                rec.leaf("a", fill(1.0));
                rec.leaf("gone", fill(3.0).collapsed());
                rec.leaf("b", fill(1.0));
            },
        );
    });
    let (a, b) = (rig.rect("a"), rig.rect("b"));
    assert_eq!((a.min.x, a.size.w), (0.0, 200.0));
    assert_eq!((b.min.x, b.size.w), (200.0, 200.0));
}
