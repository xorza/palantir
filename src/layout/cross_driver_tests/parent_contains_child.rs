//! Pin: a panel's arranged rect is at least as large as the union of
//! its children's rects on each axis (modulo negative margins, not
//! tested here). The rule is: `Fill` and `Hug` parents grow to contain
//! their children even when the parent's available space is smaller
//! than the children's measured size; `Fixed` is a hard contract and
//! does *not* grow. The root rect grows past the surface for the same
//! reason.
//!
//! Anti-regression for "showcase window resized small → parent panel
//! ends up smaller than its child."

use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::support::testing::ui_at;
use crate::tree::element::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

fn assert_contains(parent: Rect, child: Rect, label: &str) {
    assert!(
        parent.size.w + 0.5 >= child.size.w,
        "{label}: parent.w={} < child.w={}",
        parent.size.w,
        child.size.w,
    );
    assert!(
        parent.size.h + 0.5 >= child.size.h,
        "{label}: parent.h={} < child.h={}",
        parent.size.h,
        child.size.h,
    );
    assert!(
        parent.min.x <= child.min.x + 0.5
            && child.min.x + child.size.w <= parent.min.x + parent.size.w + 0.5,
        "{label}: child x-span [{}, {}] outside parent [{}, {}]",
        child.min.x,
        child.min.x + child.size.w,
        parent.min.x,
        parent.min.x + parent.size.w,
    );
    assert!(
        parent.min.y <= child.min.y + 0.5
            && child.min.y + child.size.h <= parent.min.y + parent.size.h + 0.5,
        "{label}: child y-span [{}, {}] outside parent [{}, {}]",
        child.min.y,
        child.min.y + child.size.h,
        parent.min.y,
        parent.min.y + parent.size.h,
    );
}

/// Cramped surface, Fill parent with a Fixed child wider than the
/// surface: parent grows past the surface to contain the child.
#[test]
fn fill_hstack_grows_past_surface_to_contain_fixed_child() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let mut child_node = None;
    let parent = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            child_node = Some(
                Frame::new()
                    .size((Sizing::Fixed(300.0), Sizing::Fixed(50.0)))
                    .show(ui)
                    .node,
            );
        })
        .node;
    ui.end_frame();

    let p = ui.pipeline.layout.result.rect[parent.index()];
    let c = ui.pipeline.layout.result.rect[child_node.unwrap().index()];

    assert_eq!(c.size.w, 300.0, "Fixed child stays Fixed");
    assert!(
        p.size.w >= 300.0,
        "Fill parent should grow to contain 300-wide child; got w={}",
        p.size.w,
    );
    assert_contains(p, c, "fill_hstack/fixed_child");
}

/// `Fixed` parent is a hard contract: it does *not* grow even when its
/// child is bigger. The child overflows. This documents the explicit
/// carve-out from the parent-≥-child rule. The Fixed panel here is a
/// *non-root* node so its slot follows from its declared size, not the
/// surface (the root special-case grows past the surface to contain
/// its measured content; see `root_grows_past_surface_*`).
#[test]
fn fixed_parent_does_not_grow_for_oversized_child() {
    let mut ui = ui_at(UVec2::new(800, 600));
    let mut fixed_panel = None;
    let mut child_node = None;
    Panel::vstack().show(&mut ui, |ui| {
        fixed_panel = Some(
            Panel::hstack()
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui, |ui| {
                    child_node = Some(
                        Frame::new()
                            .size((Sizing::Fixed(300.0), Sizing::Fixed(300.0)))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    ui.end_frame();

    let p = ui.pipeline.layout.result.rect[fixed_panel.unwrap().index()];
    let c = ui.pipeline.layout.result.rect[child_node.unwrap().index()];

    assert_eq!(p.size.w, 50.0, "Fixed parent stays at its declared size");
    assert_eq!(p.size.h, 50.0);
    assert_eq!(c.size.w, 300.0, "Fixed child still measured at its size");
    assert_eq!(c.size.h, 300.0);
}

/// Two Fill children inside a Fill parent in a cramped surface: each
/// child's MinContent floor (Fixed siblings here) sums past the parent's
/// available width. Parent grows; arranged inner span fits both
/// children.
#[test]
fn fill_hstack_grows_for_oversized_fixed_siblings() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let mut a_node = None;
    let mut b_node = None;
    let parent = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            a_node = Some(
                Frame::new()
                    .with_id("a")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
            b_node = Some(
                Frame::new()
                    .with_id("b")
                    .size((Sizing::Fixed(150.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
        })
        .node;
    ui.end_frame();

    let p = ui.pipeline.layout.result.rect[parent.index()];
    let a = ui.pipeline.layout.result.rect[a_node.unwrap().index()];
    let b = ui.pipeline.layout.result.rect[b_node.unwrap().index()];

    assert!(
        p.size.w >= 350.0,
        "Fill parent should grow to contain 200+150 children; got w={}",
        p.size.w,
    );
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 150.0);
    assert_contains(p, a, "fill_hstack/sibling_a");
    assert_contains(p, b, "fill_hstack/sibling_b");
    assert!(
        b.min.x >= a.min.x + a.size.w - 0.5,
        "siblings should not overlap; a.end={} b.start={}",
        a.min.x + a.size.w,
        b.min.x,
    );
}

/// Nested case: Fill parent containing a Hug panel that contains a
/// Fixed wide child. Both wrappers grow; the parent's rect contains the
/// inner panel's rect, which contains the leaf.
#[test]
fn nested_fill_hug_grows_through_intermediate_panel() {
    let mut ui = ui_at(UVec2::new(80, 200));
    let mut inner_node = None;
    let mut leaf_node = None;
    let outer = Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            let inner = Panel::hstack()
                .size((Sizing::Hug, Sizing::Hug))
                .show(ui, |ui| {
                    leaf_node = Some(
                        Frame::new()
                            .size((Sizing::Fixed(250.0), Sizing::Fixed(60.0)))
                            .show(ui)
                            .node,
                    );
                });
            inner_node = Some(inner.node);
        })
        .node;
    ui.end_frame();

    let o = ui.pipeline.layout.result.rect[outer.index()];
    let i = ui.pipeline.layout.result.rect[inner_node.unwrap().index()];
    let l = ui.pipeline.layout.result.rect[leaf_node.unwrap().index()];

    assert_eq!(l.size.w, 250.0);
    assert!(
        i.size.w >= 250.0,
        "Hug inner should hug to 250; got {}",
        i.size.w
    );
    assert!(o.size.w >= i.size.w, "Fill outer should contain Hug inner");
    assert_contains(i, l, "nested/inner_contains_leaf");
    assert_contains(o, i, "nested/outer_contains_inner");
}

/// Root rect grows past the surface when content exceeds it. The
/// audit confirmed the renderer/composer/cascade tolerate this; the
/// GPU scissor clips at the viewport.
#[test]
fn root_grows_past_surface_when_content_exceeds_it() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let root = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new()
                .size((Sizing::Fixed(400.0), Sizing::Fixed(300.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let r = ui.pipeline.layout.result.rect[root.index()];
    assert!(
        r.size.w >= 400.0,
        "root should grow past 100 px surface to contain 400 px child; got w={}",
        r.size.w,
    );
    assert!(
        r.size.h >= 300.0,
        "root should grow past 100 px surface vertically; got h={}",
        r.size.h,
    );
}

/// Pin (regression for the wrap-toolbar visual bug): a Fill-on-cross
/// child of a parent that grows past `available` (because of a
/// Fixed-content sibling) must be measured against its *post-grow*
/// inner — not the pre-grow surface-derived inner. Without two-pass
/// measure, a WrapHStack toolbar packs many rows at the narrow
/// pre-grow width (large `desired.h`) but arranges only a few at the
/// wider post-grow width, leaving a tall empty band between the
/// toolbar and its next sibling.
#[test]
fn wrap_toolbar_packs_at_post_grow_width() {
    use crate::primitives::color::Color;
    use crate::widgets::styled::Styled;
    let mut ui = ui_at(UVec2::new(150, 800));
    let mut toolbar_node = None;
    Panel::vstack()
        .with_id("root")
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .gap(12.0)
        .show(&mut ui, |ui| {
            // Toolbar: WrapHStack with several wide buttons.
            toolbar_node = Some(
                Panel::wrap_hstack()
                    .with_id("toolbar")
                    .gap(6.0)
                    .line_gap(6.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        for i in 0..16u32 {
                            Frame::new()
                                .with_id(("btn", i))
                                .size((Sizing::Fixed(80.0), Sizing::Fixed(28.0)))
                                .fill(Color::rgb(0.3, 0.3, 0.5))
                                .show(ui);
                        }
                    })
                    .node,
            );
            // Sibling: a Fill panel containing a Fixed-width Frame
            // bigger than the surface. Forces root to grow on width.
            Panel::zstack()
                .with_id("central")
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .show(ui, |ui| {
                    Frame::new()
                        .with_id("body")
                        .size((Sizing::Fixed(360.0), Sizing::Fixed(60.0)))
                        .show(ui);
                });
        });
    ui.end_frame();

    let toolbar_rect = ui.pipeline.layout.result.rect[toolbar_node.unwrap().index()];
    // Buttons are 28 px tall. With root grown so toolbar.inner.w fits
    // 4 buttons per row (80 + 6 gap each), 16 buttons → 4 rows. That's
    // 4 × 28 + 3 × 6 = 130 px. Without the two-pass measure, toolbar's
    // desired.h was computed from packing at the original narrow
    // surface width (1–2 buttons per row, ~14 rows ≈ 526 px), so
    // arrange would allocate ~526 px and leave a tall empty band.
    assert!(
        toolbar_rect.size.h < 200.0,
        "toolbar packed at post-grow width should be ~130 px tall, \
         got {} (likely the pre-grow narrow-surface packing)",
        toolbar_rect.size.h,
    );
}

/// Pin (regression for grid-section drift bug): a Hug grid containing
/// a wrapping-text cell + label cell, inside a Hug section, inside a
/// Fill chain that grows past the surface. Pre-fix: the grid's row
/// height was accumulated via `.max()` into `GridHugStore` during the
/// pre-grow first measure (with narrow column widths → tall wrapped
/// text). The post-grow second measure (wider columns → shorter
/// wrapped text) couldn't shrink the row because `.max()` kept the
/// older taller value. Section.rect.h ended up taller than the
/// rendered text, leaving a tall empty band before the next sibling
/// — the user-visible "panel drifts down" effect.
#[test]
fn two_hug_cols_section_height_matches_post_grow_text() {
    use crate::layout::types::track::Track;
    use crate::primitives::color::Color;
    use crate::widgets::grid::Grid;
    use crate::widgets::styled::Styled;
    use crate::widgets::text::Text;
    use std::rc::Rc;
    let mut ui = crate::support::testing::ui_with_text(UVec2::new(200, 1100));
    let mut section_node = None;
    let mut grid_node = None;
    let mut text_node = None;
    // Mimic showcase root → central ZStack (FILL × FILL, padding 16) →
    // text-layouts vstack (FILL × FILL, gap 16).
    Panel::vstack()
        .with_id("root")
        .size((Sizing::FILL, Sizing::FILL))
        .padding(12.0)
        .show(&mut ui, |ui| {
            Panel::zstack()
                .with_id("central")
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .show(ui, |ui| {
                    Panel::vstack()
                        .with_id("text-layouts")
                        .size((Sizing::FILL, Sizing::FILL))
                        .gap(16.0)
                        .show(ui, |ui| {
                            section_node = Some(
                                Panel::vstack()
                                    .with_id("section1")
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .padding(8.0)
                                    .gap(6.0)
                                    .fill(Color::rgb(0.16, 0.18, 0.22))
                                    .show(ui, |ui| {
                                        Text::new("two Hug columns")
                                            .with_id("title")
                                            .size_px(12.0)
                                            .show(ui);
                                        grid_node = Some(
                                            Grid::new()
                                                .with_id("grid")
                                                .cols(Rc::from([Track::hug(), Track::hug()]))
                                                .rows(Rc::from([Track::hug()]))
                                                .gap_xy(0.0, 16.0)
                                                .show(ui, |ui| {
                                                    text_node = Some(
                                        Text::new(
                                            "The quick brown fox jumps over the lazy dog. \
                                             Pack my box with five dozen liquor jugs. \
                                             How vexingly quick daft zebras jump!",
                                        )
                                        .with_id("para")
                                        .size_px(14.0)
                                        .wrapping()
                                        .grid_cell((0, 0))
                                        .show(ui)
                                        .node,
                                    );
                                                    Text::new("right column")
                                                        .with_id("right")
                                                        .size_px(14.0)
                                                        .grid_cell((0, 1))
                                                        .show(ui);
                                                })
                                                .node,
                                        );
                                    })
                                    .node,
                            );
                            // Sibling section with a Fixed-wide Frame: forces the
                            // text-layouts/central/root chain to grow past the surface
                            // (Fill-grow), which in turn re-measures section1 with a
                            // wider available — the chain that exposed the hug-store
                            // staleness bug.
                            Panel::vstack()
                                .with_id("section2")
                                .size((Sizing::FILL, Sizing::Hug))
                                .padding(8.0)
                                .fill(Color::rgb(0.16, 0.18, 0.22))
                                .show(ui, |ui| {
                                    Frame::new()
                                        .with_id("wide-frame")
                                        .size((Sizing::Fixed(400.0), Sizing::Fixed(20.0)))
                                        .show(ui);
                                });
                        });
                });
        });
    ui.end_frame();

    let section = ui.pipeline.layout.result.rect[section_node.unwrap().index()];
    let grid = ui.pipeline.layout.result.rect[grid_node.unwrap().index()];
    let text = ui.pipeline.layout.result.rect[text_node.unwrap().index()];
    let shaped = ui.pipeline.layout.result.text_shapes[text_node.unwrap().index()]
        .expect("para text was shaped");

    // Section's rect.h must match the post-grow text wrap, not the
    // pre-grow narrow-column wrap. With root growing to ~440 (to fit
    // the 400-wide Fixed sibling), section1's grid post-grow column 0
    // is ~280 px wide — text wraps to ~3 lines (~50 px), grid.h ≈ 50,
    // section.h ≈ 90. Pre-fix: hug-store kept the narrow-column row
    // height (~250+ px), section.h was 280+.
    assert!(
        section.size.h < 120.0,
        "section.h should match post-grow text height (~3 lines), got {}",
        section.size.h,
    );
    assert!(
        (grid.size.h - shaped.measured.h).abs() < 4.0,
        "grid.h ({}) should match the post-grow text-shape height ({}) — \
         hug-store stale row height kept the pre-grow narrow-column wrap",
        grid.size.h,
        shaped.measured.h,
    );
    assert!(
        text.size.w > 100.0,
        "text shaped at post-grow column width should be wide (paragraph \
         wrapped to several columns), not the narrow intrinsic-min, got w={}",
        text.size.w,
    );
}

/// Negative case: parent stays at the surface size when children fit.
/// The floor is `max(available, hug)`, so available wins when bigger.
#[test]
fn fill_parent_stays_at_available_when_children_fit() {
    let mut ui = ui_at(UVec2::new(800, 600));
    let parent = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new()
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let r = ui.pipeline.layout.result.rect[parent.index()];
    assert_eq!(r.size.w, 800.0);
    assert_eq!(r.size.h, 600.0);
}
