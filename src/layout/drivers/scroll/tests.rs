//! Driver-level tests for [`Scroll`](crate::layout::drivers::scroll::Scroll)'s
//! measure and arrange, on bare scroll nodes, as the `Scroll` widget's viewport and a
//! `TextEdit` record them.

use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder};
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

const SURFACE: Size = Size::new(400.0, 300.0);

/// A scroll node panning `axes`, sized `size`, fitting its content on each `Hug` panned axis,
/// as the `Scroll` widget builds its viewport.
fn scroll(axes: ScrollAxes, size: (Sizing, Sizing)) -> Widget {
    Widget::scroll(axes.fit_content(SizeSpec::new(size.0, size.1))).size(size)
}

/// `count` leaves of `size`, `{prefix}{i}`.
fn rows(rec: &mut NodeRecorder<'_>, prefix: &str, count: u32, size: (Sizing, Sizing)) {
    for i in 0..count {
        rec.leaf(&format!("{prefix}{i}"), Widget::leaf().size(size));
    }
}

/// Vertical scroll measures children with INF on Y; the content extent is their full height:
/// 5 × 50 = 250.
#[test]
fn vertical_scroll_records_content_extent() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let fixed = (Sizing::fixed(200.0), Sizing::fixed(100.0));
        rec.node("scroll", scroll(ScrollAxes::VERTICAL, fixed), |rec| {
            rows(rec, "row", 5, (Sizing::FILL, Sizing::fixed(50.0)));
        });
    });
    assert_eq!(rig.scroll_content("scroll").h, 250.0);
}

/// Ten 40 px columns 4 px apart: 10 × 40 + 9 × 4 = 436.
#[test]
fn horizontal_scroll_records_content_extent() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let fixed = (Sizing::fixed(200.0), Sizing::fixed(80.0));
        rec.node(
            "scroll",
            scroll(ScrollAxes::HORIZONTAL, fixed).gap(4.0),
            |rec| {
                rows(rec, "col", 10, (Sizing::fixed(40.0), Sizing::FILL));
            },
        );
    });
    assert_eq!(rig.scroll_content("scroll").w, 436.0);
}

#[test]
fn both_axis_scroll_records_content_extent() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let fixed = (Sizing::fixed(100.0), Sizing::fixed(100.0));
        rec.node("scroll", scroll(ScrollAxes::BOTH, fixed), |rec| {
            rec.leaf("wide-tall", Widget::leaf().size((300.0, 250.0)));
        });
    });
    assert_eq!(rig.scroll_content("scroll"), Size::new(300.0, 250.0));
}

/// A second run over the same nodes meets the first's measure cache and reports the same
/// viewport and content: 4 × 40 = 160.
#[test]
fn layout_output_survives_across_runs() {
    let mut rig = LayoutRig::new();
    let run = |rig: &mut LayoutRig| {
        rig.run(SURFACE, |rec| {
            let fixed = (Sizing::fixed(150.0), Sizing::fixed(100.0));
            rec.node("scroll", scroll(ScrollAxes::VERTICAL, fixed), |rec| {
                rows(rec, "row", 4, (Sizing::FILL, Sizing::fixed(40.0)));
            });
        });
        (rig.rect("scroll"), rig.scroll_content("scroll"))
    };
    let first = run(&mut rig);
    assert_eq!(run(&mut rig), first);
    assert_eq!(first.1.h, 160.0);
}

/// A rig running a `count`-row vertical **Hug** scroll (50 px rows) with the given min and max
/// heights, in a Hug vstack that isolates the assertion from how the root is arranged.
fn hug_scroll(count: u32, min_h: f32, max_h: f32) -> LayoutRig {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node("root", Widget::vstack(), |rec| {
            rec.node(
                "scroll",
                scroll(ScrollAxes::VERTICAL, (Sizing::HUG, Sizing::HUG))
                    .min_size((0.0, min_h))
                    .max_size((f32::INFINITY, max_h)),
                |rec| {
                    rows(
                        rec,
                        "row",
                        count,
                        (Sizing::fixed(120.0), Sizing::fixed(50.0)),
                    );
                },
            );
        });
    });
    rig
}

/// A `Hug` scroll sizes to its content clamped to `[min, max]`: 3 × 50 = 150 below the cap,
/// pinned to `min_size` under the floor (1 × 50 floored at 120), stopped at the cap above
/// (8 × 50 = 400 capped at 200) with the content overflowing. The content extent records the
/// rows in full, so bar and thumb size against real content.
#[test]
fn hug_scroll_clamps_viewport_to_content() {
    let cases: &[(&str, u32, f32, f32, f32)] = &[
        ("fits_content_below_max", 3, 0.0, 400.0, 150.0),
        ("floors_at_min", 1, 120.0, 400.0, 120.0),
        ("caps_at_max", 8, 0.0, 200.0, 200.0),
    ];
    for &(label, count, min_h, max_h, want) in cases {
        let rig = hug_scroll(count, min_h, max_h);
        assert_eq!(rig.rect("scroll").size.h, want, "case: {label}");
        assert_eq!(
            rig.scroll_content("scroll").h,
            count as f32 * 50.0,
            "case: {label}: content extent",
        );
    }
}

/// A `Hug` scroll under a fixed 100 px parent takes the parent's bound as its viewport while
/// its 8 × 50 = 400 of content overflows. One Hug stack further in, below a fixed 30 px header,
/// a scroll's zero floor on its panned axis lets the stack take the parent's 100, and the
/// viewport gives way to the 100 − 30 = 70 its rigid sibling leaves.
#[test]
fn hug_scroll_viewport_follows_parent_cap() {
    let capped = |rec: &mut NodeRecorder<'_>| {
        rec.node(
            "scroll",
            scroll(ScrollAxes::VERTICAL, (Sizing::HUG, Sizing::HUG)),
            |rec| rows(rec, "row", 8, (Sizing::fixed(120.0), Sizing::fixed(50.0))),
        );
    };
    let parent = || Widget::vstack().size((Sizing::fixed(200.0), Sizing::fixed(100.0)));
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| rec.node("parent", parent(), capped));
    assert_eq!(
        rig.rect("scroll").size.h,
        100.0,
        "viewport follows the parent cap"
    );
    assert_eq!(
        rig.scroll_content("scroll").h,
        400.0,
        "content keeps its natural extent"
    );

    rig.run(SURFACE, |rec| {
        rec.node("parent", parent(), |rec| {
            rec.node("hug-around-scroll", Widget::vstack(), |rec| {
                rec.leaf("header", Widget::leaf().size((120.0, 30.0)));
                capped(rec);
            });
        });
    });
    assert_eq!(
        rig.rect("hug-around-scroll").size.h,
        100.0,
        "the Hug stack shrinks to the parent cap",
    );
    assert_eq!(
        rig.rect("scroll").size.h,
        70.0,
        "the viewport takes what the header leaves"
    );
    assert_eq!(
        rig.scroll_content("scroll").h,
        400.0,
        "content keeps its natural extent"
    );
}

/// The `Hug` vstack holding one vertical scroll of `pan_h` over a 150 px row, run on `rig`.
fn scroll_in_hug_parent(rig: &mut LayoutRig, pan_h: Sizing) {
    rig.run(SURFACE, |rec| {
        rec.node("root", Widget::vstack(), |rec| {
            rec.node(
                "scroll",
                scroll(ScrollAxes::VERTICAL, (Sizing::HUG, pan_h)),
                |rec| {
                    rec.leaf("row", Widget::leaf().size((120.0, 150.0)));
                },
            );
        });
    });
}

/// Counterpart guard: a `Fill` scroll keeps the content-independent viewport and reports zero on
/// its pan axis, so it does **not** inflate a `Hug` ancestor.
#[test]
fn fill_scroll_does_not_grow_hug_parent() {
    let mut rig = LayoutRig::new();
    scroll_in_hug_parent(&mut rig, Sizing::fill(1.0));
    assert_eq!(
        rig.rect("scroll").size.h,
        0.0,
        "a Fill scroll reports zero pan-axis extent; the Hug parent doesn't grow",
    );
}

/// Toggling a scroll's pan-axis `Sizing` (`Hug` ⇄ `Fill`) on the **same id** busts the
/// `MeasureCache`: the fit bits ride the scroll specification, which is folded into the
/// subtree hash. Without it the second run would be served the first's stale 150.
#[test]
fn toggling_scroll_sizing_busts_measure_cache() {
    let mut rig = LayoutRig::new();
    scroll_in_hug_parent(&mut rig, Sizing::HUG);
    assert_eq!(rig.rect("scroll").size.h, 150.0, "Hug fits its content");
    scroll_in_hug_parent(&mut rig, Sizing::fill(1.0));
    assert_eq!(
        rig.rect("scroll").size.h,
        0.0,
        "Fill collapses in the Hug parent — the first run's fit measure is not served stale",
    );
}

/// Pin: a `Hug` scroll reports its content extent as its **intrinsic**, not just its measured
/// size. If the intrinsic query ignored the `fit` bit and answered zero, nothing downstream of
/// `measure` would notice (`AxisSlot::resolve` takes `max(content, intrinsic_min)`), but a Hug
/// grid column, resolved from the intrinsic walk, would resolve to zero and its cell
/// would overflow.
#[test]
fn hug_scroll_drives_the_hug_grid_column_it_sits_in() {
    const CONTENT_W: f32 = 120.0;

    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        let tracks = GridTracks {
            rows: &[Track::HUG],
            cols: &[Track::HUG],
        };
        rec.grid("grid", Widget::grid(), tracks, |rec| {
            rec.node(
                "hug-scroll",
                scroll(ScrollAxes::HORIZONTAL, (Sizing::HUG, Sizing::fixed(40.0))),
                |rec| rec.leaf("wide", Widget::leaf().size((CONTENT_W, 20.0))),
            );
        });
    });
    assert_eq!(
        rig.rect("hug-scroll").size.w,
        CONTENT_W,
        "a Hug column must resolve to the Hug scroll's content width, not collapse to zero",
    );
}

/// **A scroll viewport takes the slot it is placed in, whichever driver places it.** Its
/// `desired` follows the content, so a bounded parent can hand it a smaller slot; the viewport
/// clips, so it must not overflow that slot as an ordinary node does.
///
/// One case per placing driver: a bare scroll node in a Grid or a stack's cross axis needs the
/// clamp too, and `TextEdit` is such a node. A stack's *main* axis is the one placement
/// `AxisPlacement` does not own (its flex solver shrinks against the zero min-content a panned
/// scroll reports). Canvas is absent: its slot is the child's desired on a `Hug` axis and
/// `inner` on a sized one, so the clamp is the identity.
#[test]
fn a_scroll_viewport_takes_its_slot_under_every_driver_that_places_one() {
    const SLOT: Size = Size { w: 200.0, h: 100.0 };
    const CONTENT: Size = Size { w: 400.0, h: 400.0 };

    let viewport = |rec: &mut NodeRecorder<'_>| {
        rec.node(
            "scroll",
            scroll(ScrollAxes::BOTH, (Sizing::HUG, Sizing::HUG)),
            |rec| rec.leaf("content", Widget::leaf().size((CONTENT.w, CONTENT.h))),
        );
    };
    for driver in ["zstack", "hstack", "vstack", "grid"] {
        let mut rig = LayoutRig::new();
        rig.run(SURFACE, |rec| {
            // **Hug capped by `max_size`, not `Fixed`.** A Hug axis measures children against
            // `INFINITY`, so the cap makes the slot smaller than the desired; a Fixed parent
            // bounds the measure instead and never reaches this placement.
            let cap = |widget: Widget| widget.max_size((SLOT.w, SLOT.h));
            match driver {
                "zstack" => rec.node("parent", cap(Widget::zstack()), viewport),
                "hstack" => rec.node("parent", cap(Widget::hstack()), viewport),
                "vstack" => rec.node("parent", cap(Widget::vstack()), viewport),
                _ => {
                    let tracks = GridTracks {
                        rows: &[Track::HUG],
                        cols: &[Track::HUG],
                    };
                    rec.grid("parent", cap(Widget::grid()), tracks, viewport);
                }
            }
        });
        assert_eq!(
            rig.rect("scroll").size,
            SLOT,
            "{driver}: a viewport measuring {CONTENT:?} must still take its {SLOT:?} slot",
        );
        assert_eq!(
            rig.scroll_content("scroll"),
            CONTENT,
            "{driver}: and still record the full content extent for its bars",
        );
    }
}
