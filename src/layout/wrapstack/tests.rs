use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Justify, Rect, Sizing};
use crate::tree::NodeId;
use crate::widgets::{Frame, Panel, Styled};

fn cell(ui: &mut Ui, id: &'static str, w: f32, h: f32) -> NodeId {
    Frame::with_id(id)
        .size((Sizing::Fixed(w), Sizing::Fixed(h)))
        .fill(Color::WHITE)
        .show(ui)
        .node
}

/// Wrap a `WrapHStack`/`WrapVStack` under test inside an outer Fill
/// HStack so its own Fixed/Hug sizing isn't overridden by the root
/// surface. Same trick canvas/zstack tests use.
fn under_outer<F: FnOnce(&mut Ui) -> NodeId>(ui: &mut Ui, surface: Rect, f: F) -> NodeId {
    ui.begin_frame();
    let mut inner = None;
    Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .clip(false)
        .show(ui, |ui| {
            inner = Some(f(ui));
        });
    ui.layout(surface);
    inner.unwrap()
}

/// Pin: three 60×20 cells in a 200-wide WrapHStack with `gap=10` fit on
/// one line (60+10+60+10+60 = 200). All three sit at y=0.
#[test]
fn wrap_hstack_packs_into_single_line_when_fits() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, Rect::new(0.0, 0.0, 400.0, 400.0), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
                kids.push(cell(ui, "c", 60.0, 20.0));
            })
            .node
    });
    let a = ui.rect(kids[0]);
    let b = ui.rect(kids[1]);
    let c = ui.rect(kids[2]);
    assert_eq!(a.min.y, 0.0);
    assert_eq!(b.min.y, 0.0);
    assert_eq!(c.min.y, 0.0);
    assert_eq!(a.min.x, 0.0);
    assert_eq!(b.min.x, 70.0);
    assert_eq!(c.min.x, 140.0);
}

/// Pin: a 4th 60-wide cell wraps to a new line because 60+10+60+10+60+10+60 = 250 > 200.
/// Lines have height 20; line_gap=8 → 4th cell sits at y=28.
#[test]
fn wrap_hstack_wraps_when_next_child_overflows() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, Rect::new(0.0, 0.0, 400.0, 400.0), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
                kids.push(cell(ui, "c", 60.0, 20.0));
                kids.push(cell(ui, "d", 60.0, 20.0));
            })
            .node
    });
    let a = ui.rect(kids[0]);
    let b = ui.rect(kids[1]);
    let c = ui.rect(kids[2]);
    let d = ui.rect(kids[3]);
    assert_eq!((a.min.x, a.min.y), (0.0, 0.0));
    assert_eq!((b.min.x, b.min.y), (70.0, 0.0));
    assert_eq!((c.min.x, c.min.y), (140.0, 0.0));
    assert_eq!((d.min.x, d.min.y), (0.0, 28.0));
}

/// Pin: when a child is wider than the available main, it sits alone on
/// its line (no infinite recursion, no wrapping inside the child).
#[test]
fn wrap_hstack_oversize_child_owns_its_line() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, Rect::new(0.0, 0.0, 400.0, 400.0), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(100.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "small", 50.0, 20.0));
                kids.push(cell(ui, "wide", 200.0, 20.0));
                kids.push(cell(ui, "tail", 50.0, 20.0));
            })
            .node
    });
    let small = ui.rect(kids[0]);
    let wide = ui.rect(kids[1]);
    let tail = ui.rect(kids[2]);
    // line 0: small alone (50+10+200 > 100, wide overflows → wraps)
    assert_eq!((small.min.x, small.min.y), (0.0, 0.0));
    // line 1: wide alone (overflowed)
    assert_eq!((wide.min.x, wide.min.y), (0.0, 28.0));
    // line 2: tail
    assert_eq!((tail.min.x, tail.min.y), (0.0, 56.0));
}

/// Pin: line height = max child cross within the line; subsequent
/// lines start at the previous line's bottom + `line_gap`.
#[test]
fn wrap_hstack_line_height_is_max_child_cross() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, Rect::new(0.0, 0.0, 400.0, 400.0), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(0.0)
            .line_gap(0.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "tall", 100.0, 60.0));
                kids.push(cell(ui, "short", 100.0, 20.0));
                // overflow → new line
                kids.push(cell(ui, "next-line", 100.0, 30.0));
            })
            .node
    });
    let tall = ui.rect(kids[0]);
    let short = ui.rect(kids[1]);
    let next = ui.rect(kids[2]);
    assert_eq!(tall.min.y, 0.0);
    assert_eq!(short.min.y, 0.0);
    // Line 0 height = 60; line_gap = 0 → next at y=60.
    assert_eq!(next.min.y, 60.0);
}

/// Pin: `Justify::Center` per-line. With a 200-wide WrapHStack and line
/// content 60+10+60 = 130, leftover = 70 → start_offset = 35.
#[test]
fn wrap_hstack_justify_center_per_line() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, Rect::new(0.0, 0.0, 400.0, 400.0), |ui| {
        Panel::wrap_hstack_with_id("w")
            .size((Sizing::Fixed(200.0), Sizing::Hug))
            .gap(10.0)
            .line_gap(0.0)
            .justify(Justify::Center)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 60.0, 20.0));
                kids.push(cell(ui, "b", 60.0, 20.0));
            })
            .node
    });
    let a = ui.rect(kids[0]);
    let b = ui.rect(kids[1]);
    assert_eq!(a.min.x, 35.0);
    assert_eq!(b.min.x, 105.0);
}

/// Pin: WrapVStack — same code via `Axis::Y`. Children flow top-to-
/// bottom, wrap to new column on the right.
#[test]
fn wrap_vstack_wraps_columns_when_main_overflows() {
    let mut ui = Ui::new();
    let mut kids = Vec::new();
    let _wrap = under_outer(&mut ui, Rect::new(0.0, 0.0, 400.0, 400.0), |ui| {
        Panel::wrap_vstack_with_id("w")
            .size((Sizing::Hug, Sizing::Fixed(100.0)))
            .gap(10.0)
            .line_gap(8.0)
            .show(ui, |ui| {
                kids.push(cell(ui, "a", 20.0, 40.0));
                kids.push(cell(ui, "b", 20.0, 40.0));
                // 40+10+40+10+40 = 140 > 100 → c wraps
                kids.push(cell(ui, "c", 20.0, 40.0));
            })
            .node
    });
    let a = ui.rect(kids[0]);
    let b = ui.rect(kids[1]);
    let c = ui.rect(kids[2]);
    // Column 0: a, b at x=0.
    assert_eq!((a.min.x, a.min.y), (0.0, 0.0));
    assert_eq!((b.min.x, b.min.y), (0.0, 50.0));
    // Column 1: c at x=20+8=28, y=0.
    assert_eq!((c.min.x, c.min.y), (28.0, 0.0));
}

/// Pin: a Fixed-width Hug-height WrapHStack hugs to its packed cross
/// extent. 4 cells of 60×20 in a 200-wide wrap: 3 fit on line 0
/// (60+10+60+10+60 = 190), 1 wraps to line 1. Outer h = 20+8+20 = 48.
///
/// Note: a fully-Hug WrapHStack (no main constraint anywhere up the
/// chain) collapses to a single line — intrinsic measure runs at
/// `INF` main with no width to wrap against. To force wrap, the
/// WrapHStack (or some ancestor) must commit a finite main size.
#[test]
fn wrap_hstack_with_fixed_main_hugs_cross_to_packed_lines() {
    let mut ui = Ui::new();
    let mut wrap_node = None;
    let _wrap = under_outer(&mut ui, Rect::new(0.0, 0.0, 400.0, 400.0), |ui| {
        wrap_node = Some(
            Panel::wrap_hstack_with_id("w")
                .size((Sizing::Fixed(200.0), Sizing::Hug))
                .gap(10.0)
                .line_gap(8.0)
                .show(ui, |ui| {
                    cell(ui, "a", 60.0, 20.0);
                    cell(ui, "b", 60.0, 20.0);
                    cell(ui, "c", 60.0, 20.0);
                    cell(ui, "d", 60.0, 20.0);
                })
                .node,
        );
        wrap_node.unwrap()
    });
    let r = ui.rect(wrap_node.unwrap());
    assert_eq!(r.size.w, 200.0, "Fixed main width is honored");
    // Two lines of 20 + 8 line_gap = 48.
    assert_eq!(r.size.h, 48.0);
}
