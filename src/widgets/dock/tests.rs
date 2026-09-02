//! The dock tree's invariants: the six ops, the re-pack every
//! structural change ends with, the depth cap, and what `validate`
//! refuses. Plus the pane geometry one recorded frame produces.

use glam::{UVec2, Vec2};

use crate::layout::types::sizing::Sizing;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::rect::Rect;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::ui::harness::UiHarness;
use crate::widgets::dock::allowed_splits::AllowedSplits;
use crate::widgets::dock::dock_node::{DockNode, DockSplit, NodeIdx};
use crate::widgets::dock::dock_op::{DockDrop, DockOp};
use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::dock_state::DockState;
use crate::widgets::dock::dock_tabs::DockTabs;
use crate::widgets::dock::dock_view::DockView;
use crate::widgets::dock::pane_geometry::PaneGeometry;
use crate::widgets::dock::split_side::{SplitDir, SplitSide};
use crate::widgets::dock::tab_group::{TabGroup, TabGroupId};
use crate::widgets::panel::Panel;
use crate::widgets::tabs::tab_item::TabBadge;
use crate::widgets::tabs::tab_strip::TabStrip;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum Tab {
    Main,
    Prefs,
    Viewer(u32),
}

fn viewer(n: u32) -> Tab {
    Tab::Viewer(n)
}

/// One pane holding `Main`, plus `Prefs` and one viewer.
fn seeded() -> DockState<Tab> {
    let mut d = DockState::new("test.dock", Tab::Main);
    let primary = d.primary().id;
    d.find_or_insert(Tab::Prefs, primary);
    d.find_or_insert(viewer(1), primary);
    d
}

/// The root as a split, with its two children resolved.
#[derive(Debug)]
struct RootSplit<'a> {
    split: DockSplit,
    first: &'a DockNode<Tab>,
    second: &'a DockNode<Tab>,
}

fn root_split(d: &DockState<Tab>) -> RootSplit<'_> {
    let DockNode::Split(split) = d.node(DockState::<Tab>::ROOT) else {
        panic!("root is a split");
    };
    RootSplit {
        split: *split,
        first: d.node(split.first),
        second: d.node(split.second),
    }
}

fn group_of(d: &DockState<Tab>, id: TabGroupId) -> &TabGroup<Tab> {
    d.group(id).expect("group is live")
}

fn split_off(d: &mut DockState<Tab>, tab: Tab, group: TabGroupId, side: SplitSide) {
    d.apply(DockOp::MoveTab {
        tab,
        to: DockDrop::Split { group, side },
    });
}

#[test]
fn a_new_dock_is_a_single_pinned_group() {
    let d = DockState::new("test.dock", Tab::Main);
    d.validate().unwrap();
    assert_eq!(d.groups().count(), 1);
    assert_eq!(d.primary().tabs, [Tab::Main]);
    assert_eq!(d.focused(), d.primary().id);
    assert_eq!(d.all_tabs().collect::<Vec<_>>(), [Tab::Main]);
    assert_eq!(d.pinned(), Tab::Main);
}

/// A refused close leaves the tree alone: the pinned tab never closes,
/// and a tab that is not open anywhere resolves to nothing.
#[test]
fn close_is_dropped_for_the_pinned_tab_or_one_that_is_not_open() {
    let mut d = seeded();
    let before = d.clone();
    d.apply(DockOp::CloseTab { tab: Tab::Main });
    d.apply(DockOp::CloseTab { tab: viewer(99) });
    assert_eq!(d, before, "neither op removed a tab");
}

/// The invariant the whole click path rests on. An op is built from one
/// frame's chip response and applied a phase later, with the strip able
/// to rearrange in between. Because an op names its *tab* rather than
/// its slot, the rearrangement cannot redirect it onto whatever slid
/// into that slot.
#[test]
fn tab_ops_follow_their_tab_across_a_rearrangement() {
    let mut d = seeded();
    let primary = d.primary().id;
    assert_eq!(
        group_of(&d, primary).tabs,
        [Tab::Main, Tab::Prefs, viewer(1)]
    );

    // Built while the viewer sits at slot 2, applied after `Prefs` left
    // and the viewer slid down to slot 1.
    let close_viewer = DockOp::CloseTab { tab: viewer(1) };
    d.apply(DockOp::CloseTab { tab: Tab::Prefs });
    assert_eq!(group_of(&d, primary).tabs, [Tab::Main, viewer(1)]);

    d.apply(close_viewer);
    assert_eq!(
        group_of(&d, primary).tabs,
        [Tab::Main],
        "the op closed the viewer, not whatever now occupies slot 2"
    );
}

/// Pointer-driven focus moves `focused` and nothing else, and a group
/// that has gone since the press was read leaves it where it was rather
/// than stranding a dead id that would fail validation at the next save.
#[test]
fn focus_moves_only_the_focused_group() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Right);
    let fresh = d.focused();
    assert_ne!(fresh, primary, "the new pane took focus");
    let actives: Vec<Tab> = d.active_tabs().collect();

    d.apply(DockOp::FocusPane { group: primary });
    d.validate().unwrap();
    assert_eq!(d.focused(), primary);
    assert_eq!(
        d.active_tabs().collect::<Vec<_>>(),
        actives,
        "focus alone moved — no pane switched tabs"
    );

    d.apply(DockOp::FocusPane { group: primary });
    assert_eq!(d.focused(), primary, "re-focusing the same pane is inert");

    let gone = d.absent_group();
    d.apply(DockOp::FocusPane { group: gone });
    assert_eq!(
        d.focused(),
        primary,
        "a group that is gone leaves focus where it was"
    );
    d.validate().unwrap();
}

/// `OpenTab` is "show me X" whole: it lands the tab in the focused group
/// only when it is not open already, and otherwise reuses — and focuses
/// — whichever pane holds it.
#[test]
fn open_tab_reuses_an_existing_tab_and_focuses_its_pane() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Right);
    let fresh = d.focused();

    d.apply(DockOp::FocusPane { group: primary });
    d.apply(DockOp::OpenTab { tab: viewer(1) });
    assert_eq!(
        d.all_tabs().collect::<Vec<_>>(),
        [Tab::Main, Tab::Prefs, viewer(1)],
        "no duplicate tab was inserted into the focused group"
    );
    assert_eq!(d.focused(), fresh, "focus followed the tab to its pane");
    assert_eq!(group_of(&d, fresh).active_tab(), viewer(1));

    d.apply(DockOp::OpenTab { tab: viewer(2) });
    assert_eq!(
        group_of(&d, fresh).tabs,
        [viewer(1), viewer(2)],
        "the new tab joined the focused pane"
    );
    assert_eq!(group_of(&d, fresh).active_tab(), viewer(2));
    d.validate().unwrap();
}

#[test]
fn split_move_and_collapse_roundtrip() {
    let mut d = seeded();
    let primary = d.primary().id;

    // Split the viewer off to the right: a Row split, primary first, the
    // new single-tab group second and focused. The re-packed vector is
    // pre-order `[split, primary, new]`, which validation pins.
    split_off(&mut d, viewer(1), primary, SplitSide::Right);
    d.validate().unwrap();
    let root = root_split(&d);
    assert_eq!(root.split.dir, SplitDir::Row);
    assert_eq!(root.split.ratio, 0.5);
    let DockNode::Group(first) = root.first else {
        panic!("primary stays first for a Right split");
    };
    assert_eq!(first.id, primary);
    assert_eq!(first.tabs, [Tab::Main, Tab::Prefs]);
    let DockNode::Group(second) = root.second else {
        panic!("the new pane is a group");
    };
    assert_eq!(second.tabs, [viewer(1)]);
    assert_eq!(d.focused(), second.id, "the new pane takes focus");

    // Moving the tab back into the primary strip collapses the split.
    d.apply(DockOp::MoveTab {
        tab: viewer(1),
        to: DockDrop::Into {
            group: primary,
            index: 1,
        },
    });
    d.validate().unwrap();
    assert!(
        matches!(d.node(DockState::<Tab>::ROOT), DockNode::Group(_)),
        "the split collapsed"
    );
    assert_eq!(
        d.primary().tabs,
        [Tab::Main, viewer(1), Tab::Prefs],
        "the tab was re-inserted at the requested index"
    );
    assert_eq!(d.primary().active, 1, "the moved tab becomes active");
    assert_eq!(d.focused(), primary, "focus follows the destination");
}

#[test]
fn left_and_top_splits_put_the_new_pane_first() {
    for (side, dir) in [
        (SplitSide::Left, SplitDir::Row),
        (SplitSide::Top, SplitDir::Column),
    ] {
        let mut d = seeded();
        let primary = d.primary().id;
        split_off(&mut d, viewer(1), primary, side);
        d.validate().unwrap();
        let root = root_split(&d);
        assert_eq!(root.split.dir, dir);
        let DockNode::Group(first) = root.first else {
            panic!("the first child is the new pane");
        };
        assert_eq!(first.tabs, [viewer(1)], "{side:?} puts the new pane first");
    }
}

#[test]
fn degenerate_and_forbidden_moves_change_nothing() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Right);
    let lone = d.focused();
    let before = d.clone();

    // A lone tab split off its own group would re-create the same shape.
    split_off(&mut d, viewer(1), lone, SplitSide::Bottom);
    assert_eq!(d, before, "a lone-tab self-split is a no-op");

    // A vanished target group is a no-op, not a panic.
    let gone = d.absent_group();
    d.apply(DockOp::MoveTab {
        tab: Tab::Prefs,
        to: DockDrop::Into {
            group: gone,
            index: 0,
        },
    });
    assert_eq!(d, before, "an unknown drop target is ignored");
}

#[test]
fn closing_the_last_tab_collapses_and_refocuses() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Bottom);
    d.apply(DockOp::CloseTab { tab: viewer(1) });
    d.validate().unwrap();
    assert!(
        matches!(d.node(DockState::<Tab>::ROOT), DockNode::Group(_)),
        "the emptied pane collapsed"
    );
    assert_eq!(
        d.focused(),
        primary,
        "a dangling focus falls back to the primary group"
    );
}

#[test]
fn close_keeps_active_on_a_surviving_tab() {
    let mut d = seeded();
    d.apply(DockOp::ActivateTab { tab: viewer(1) });
    assert_eq!(d.primary().active_tab(), viewer(1));

    // Closing the active last tab clamps `active` onto the previous one.
    d.apply(DockOp::CloseTab { tab: viewer(1) });
    d.validate().unwrap();
    assert_eq!(d.primary().tabs, [Tab::Main, Tab::Prefs]);
    assert_eq!(d.primary().active, 1);

    // Closing a tab left of the active one shifts what `active` points
    // at; per-group clamping keeps it in range.
    let mut d = seeded();
    d.apply(DockOp::ActivateTab { tab: viewer(1) });
    d.apply(DockOp::CloseTab { tab: Tab::Prefs });
    d.validate().unwrap();
    assert_eq!(d.primary().active, 1, "clamped into range");
}

#[test]
fn a_same_group_reorder_uses_pre_move_indices() {
    // Strip `[Main, Prefs, v1, v2]`; every index below is a slot in
    // *that* strip, the way drop-zone arithmetic over the visible chips
    // computes it.
    let reordered = |from: Tab, index: usize| {
        let mut d = seeded();
        let primary = d.primary().id;
        d.find_or_insert(viewer(2), primary);
        d.apply(DockOp::MoveTab {
            tab: from,
            to: DockDrop::Into {
                group: primary,
                index,
            },
        });
        d.validate().unwrap();
        d.primary().tabs.clone()
    };

    // Rightward: "Prefs before v2" (slot 3) must not overshoot to the
    // end just because the removal of Prefs shifted v2 left.
    assert_eq!(
        reordered(Tab::Prefs, 3),
        [Tab::Main, viewer(1), Tab::Prefs, viewer(2)]
    );
    // Leftward needs no compensation: "v2 before Prefs" (slot 1).
    assert_eq!(
        reordered(viewer(2), 1),
        [Tab::Main, viewer(2), Tab::Prefs, viewer(1)]
    );
    // Past the end clamps to an append.
    assert_eq!(
        reordered(Tab::Prefs, 99),
        [Tab::Main, viewer(1), viewer(2), Tab::Prefs]
    );
}

#[test]
fn dock_path_packs_distinct_addresses() {
    // Sibling and cross-depth addresses never alias: the sentinel bit
    // keeps `[first]` (0b10), `[second]` (0b11), `[first, first]`
    // (0b100) and the root (0b1) all distinct.
    let paths = [
        DockPath::ROOT,
        DockPath::ROOT.first(),
        DockPath::ROOT.second(),
        DockPath::ROOT.first().first(),
        DockPath::ROOT.first().second(),
        DockPath::ROOT.second().first(),
        DockPath::ROOT.second().second(),
    ];
    for (i, a) in paths.iter().enumerate() {
        for b in &paths[i + 1..] {
            assert_ne!(a, b, "packed paths must not alias");
        }
    }
    assert_eq!(
        DockPath::ROOT
            .second()
            .first()
            .directions()
            .collect::<Vec<_>>(),
        [true, false],
        "turns replay in root-to-leaf order"
    );
    assert_eq!(DockPath::ROOT.directions().count(), 0);
    assert_eq!(DockPath::default(), DockPath::ROOT);
}

#[test]
fn set_ratio_clamps_and_survives_stale_paths() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Right);

    d.apply(DockOp::SetRatio {
        split: DockPath::ROOT,
        ratio: 0.7,
    });
    assert_eq!(root_split(&d).split.ratio, 0.7);

    d.apply(DockOp::SetRatio {
        split: DockPath::ROOT,
        ratio: 0.01,
    });
    assert_eq!(
        root_split(&d).split.ratio,
        DockState::<Tab>::RATIO_MIN,
        "the ratio clamps to the floor"
    );

    // Paths landing on a group, or walking past a leaf, are ignored.
    d.apply(DockOp::SetRatio {
        split: DockPath::ROOT.first(),
        ratio: 0.5,
    });
    d.apply(DockOp::SetRatio {
        split: DockPath::ROOT.first().second(),
        ratio: 0.5,
    });
    d.validate().unwrap();
    assert_eq!(
        root_split(&d).split.ratio,
        DockState::<Tab>::RATIO_MIN,
        "stale paths change nothing"
    );
}

#[test]
fn split_depth_is_capped_without_losing_the_tab() {
    let mut d = seeded();
    let primary = d.primary().id;
    for n in 2..=5 {
        d.find_or_insert(viewer(n), primary);
    }

    // Chain splits off the freshly focused group: each nests one level
    // deeper, up to the default cap of four.
    let mut target = primary;
    for n in 1..=4 {
        split_off(&mut d, viewer(n), target, SplitSide::Right);
        target = d.focused();
    }
    d.validate().unwrap();
    assert_eq!(d.groups().count(), 5);
    assert!(!d.can_split(target), "the chain reached the cap");

    // The fifth split would nest past the cap: refused outright, so the
    // tree is untouched and the tab stays where it was.
    let before = d.clone();
    split_off(&mut d, viewer(5), target, SplitSide::Bottom);
    assert_eq!(d, before, "an over-deep split is a no-op");
    assert!(
        d.primary().tabs.contains(&viewer(5)),
        "the refused split must not lose the tab"
    );
}

/// A lower cap refuses sooner, and the refusal is the *model's*, not
/// only the widget's — the two knobs have to move together or a drag
/// would offer a drop `apply` then dropped on the floor.
#[test]
fn a_lower_cap_and_a_narrower_split_policy_both_refuse() {
    let mut d = DockState::new("test.dock", Tab::Main).max_depth(1);
    let primary = d.primary().id;
    d.find_or_insert(Tab::Prefs, primary);
    d.find_or_insert(viewer(1), primary);
    split_off(&mut d, Tab::Prefs, primary, SplitSide::Right);
    let target = d.focused();
    let before = d.clone();
    split_off(&mut d, viewer(1), target, SplitSide::Right);
    assert_eq!(d, before, "depth 1 refuses the second level");

    let mut d = DockState::new("test.dock", Tab::Main).allowed_splits(AllowedSplits::Row);
    let primary = d.primary().id;
    d.find_or_insert(Tab::Prefs, primary);
    let before = d.clone();
    split_off(&mut d, Tab::Prefs, primary, SplitSide::Bottom);
    assert_eq!(d, before, "a column split is refused under Row");
    split_off(&mut d, Tab::Prefs, primary, SplitSide::Right);
    assert_eq!(d.groups().count(), 2, "a row split is still offered");
}

#[test]
fn retain_prunes_across_groups_and_collapses() {
    let mut d = seeded();
    let primary = d.primary().id;
    d.find_or_insert(viewer(2), primary);
    split_off(&mut d, viewer(2), primary, SplitSide::Right);

    d.retain_tabs(|t| t != viewer(2));
    d.validate().unwrap();
    assert!(matches!(d.node(DockState::<Tab>::ROOT), DockNode::Group(_)));
    assert_eq!(
        d.all_tabs().collect::<Vec<_>>(),
        [Tab::Main, Tab::Prefs, viewer(1)]
    );
}

#[test]
fn nested_splits_stay_canonical() {
    let mut d = seeded();
    let primary = d.primary().id;
    d.find_or_insert(viewer(2), primary);
    split_off(&mut d, viewer(1), primary, SplitSide::Right);
    let right = d.focused();
    split_off(&mut d, viewer(2), right, SplitSide::Bottom);
    d.validate().unwrap();
    assert_eq!(d.groups().count(), 3);
    assert_eq!(
        d.all_tabs().collect::<Vec<_>>(),
        [Tab::Main, Tab::Prefs, viewer(1), viewer(2)],
        "pane order is left to right, top to bottom"
    );

    // Collapse the inner split; the outer one survives.
    d.apply(DockOp::MoveTab {
        tab: viewer(2),
        to: DockDrop::Into {
            group: primary,
            index: 99,
        },
    });
    d.validate().unwrap();
    assert_eq!(d.groups().count(), 2);
    let DockNode::Group(second) = root_split(&d).second else {
        panic!("the inner split dissolved into the surviving group");
    };
    assert_eq!(second.tabs, [viewer(1)]);
    assert_eq!(
        d.primary().tabs,
        [Tab::Main, Tab::Prefs, viewer(2)],
        "the clamped index appended the tab"
    );
}

#[test]
fn serde_roundtrips_through_ron() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Bottom);
    let text = ron::ser::to_string(&d).expect("serialize");
    let back: DockState<Tab> = ron::from_str(&text).expect("deserialize");
    assert_eq!(back, d);
}

#[test]
fn validate_rejects_each_corruption() {
    // Base: `[split, primary(Main, Prefs), viewer-pane(v1)]`, corrupted
    // one invariant at a time through the gated raw access — no public
    // op can produce these states.
    let base = {
        let mut d = seeded();
        let primary = d.primary().id;
        split_off(&mut d, viewer(1), primary, SplitSide::Right);
        d.validate().unwrap();
        d
    };

    type Corrupt = fn(&mut DockState<Tab>);
    let cases: [(&str, Corrupt, &str); 9] = [
        (
            "duplicate group id",
            |d| {
                let pid = d.primary().id;
                for n in d.nodes_mut() {
                    if let DockNode::Group(g) = n {
                        g.id = pid;
                    }
                }
            },
            "appears twice",
        ),
        (
            "dangling focused",
            |d| {
                let gone = d.absent_group();
                d.set_focused_unchecked(gone);
            },
            "focused group",
        ),
        (
            "active out of range",
            |d| {
                let DockNode::Group(g) = &mut d.nodes_mut()[1] else {
                    panic!("slot 1 is the primary group");
                };
                g.active = g.tabs.len();
            },
            "active tab out of range",
        ),
        (
            "ratio out of bounds",
            |d| {
                let DockNode::Split(s) = &mut d.nodes_mut()[0] else {
                    panic!("the root is a split");
                };
                s.ratio = 0.95;
            },
            "split ratio",
        ),
        (
            "children out of pre-order",
            |d| {
                let DockNode::Split(s) = &mut d.nodes_mut()[0] else {
                    panic!("the root is a split");
                };
                std::mem::swap(&mut s.first, &mut s.second);
            },
            "canonical pre-order",
        ),
        (
            "child index past the end",
            |d| d.nodes_mut().truncate(2),
            "dock node index",
        ),
        (
            "unreachable trailing slot",
            |d| {
                let DockNode::Split(s) = &mut d.nodes_mut()[0] else {
                    panic!("the root is a split");
                };
                // The root then points only at slot 1; slots 2.. are
                // orphaned.
                *s = DockSplit {
                    first: NodeIdx(1),
                    second: NodeIdx(1),
                    ..*s
                };
            },
            "canonical pre-order",
        ),
        (
            "empty group",
            |d| {
                let DockNode::Group(g) = &mut d.nodes_mut()[2] else {
                    panic!("slot 2 is the split-off viewer pane");
                };
                g.tabs.clear();
            },
            "is empty",
        ),
        (
            "no pinned tab anywhere",
            |d| {
                let DockNode::Group(g) = &mut d.nodes_mut()[1] else {
                    panic!("slot 1 is the primary group");
                };
                g.tabs.retain(|t| *t != Tab::Main);
                g.active = 0;
            },
            "no group holds the pinned tab",
        ),
    ];
    for (name, corrupt, expected) in cases {
        let mut d = base.clone();
        corrupt(&mut d);
        let err = d.validate().unwrap_err().to_string();
        assert!(err.contains(expected), "{name}: unexpected error: {err}");
    }
}

/// A duplicate tab is refused too. Split out from the table above
/// because it needs a *second* group to put the copy in, so the
/// corruption is a push rather than an edit in place.
#[test]
fn validate_rejects_a_tab_that_appears_twice() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Right);
    let DockNode::Group(g) = &mut d.nodes_mut()[1] else {
        panic!("slot 1 is the primary group");
    };
    g.tabs.push(viewer(1));
    let err = d.validate().unwrap_err().to_string();
    assert!(err.contains("appears twice"), "unexpected error: {err}");
}

/// The classification is pure rectangle arithmetic, so it is checked
/// against hand-computed geometry rather than through a recorded frame.
///
/// The pane is `(0, 0)` to `(400, 300)` with a 30 px strip on top, so
/// the content is `(0, 30)` to `(400, 300)`. At `edge_fraction = 0.25`
/// the join box is `(100, 97.5)` to `(300, 232.5)`.
#[test]
fn a_drop_classifies_into_strip_slots_join_and_wedges() {
    let group = TabGroupId(7);
    let pane = Rect::new(0.0, 0.0, 400.0, 300.0);
    let strip = Rect::new(0.0, 0.0, 400.0, 30.0);
    let chips = [
        Rect::new(6.0, 4.0, 60.0, 24.0),
        Rect::new(69.0, 4.0, 60.0, 24.0),
    ];
    let geometry = |can_split, allowed| PaneGeometry {
        group,
        pane,
        strip,
        chips: &chips,
        can_split,
        allowed,
        edge_fraction: 0.25,
        caret_width: 3.0,
    };

    // Chip centres sit at x = 36 and x = 99, so a pointer at 50 is past
    // the first and short of the second.
    let hit = geometry(true, AllowedSplits::All).classify(Vec2::new(50.0, 15.0));
    assert_eq!(hit.drop, DockDrop::Into { group, index: 1 });
    assert!(
        (hit.highlight.min.x - (69.0 - 1.5 - 1.5)).abs() < 1e-4,
        "the caret straddles the second chip's leading edge: {:?}",
        hit.highlight,
    );
    assert_eq!(hit.highlight.size.w, 3.0);

    // The inner box appends to the strip and highlights the whole
    // content rect.
    let hit = geometry(true, AllowedSplits::All).classify(Vec2::new(200.0, 165.0));
    assert_eq!(hit.drop, DockDrop::Into { group, index: 2 });
    assert_eq!(hit.highlight, Rect::new(0.0, 30.0, 400.0, 270.0));

    // Near the left edge: normalised distance picks Left over Top even
    // though the pane is wider than it is tall.
    let hit = geometry(true, AllowedSplits::All).classify(Vec2::new(20.0, 60.0));
    assert_eq!(
        hit.drop,
        DockDrop::Split {
            group,
            side: SplitSide::Left
        }
    );
    assert_eq!(hit.highlight, Rect::new(0.0, 30.0, 200.0, 270.0));

    // At the cap, and under a policy that forbids the nearest edge,
    // every wedge degrades to a join.
    let capped = geometry(false, AllowedSplits::All).classify(Vec2::new(20.0, 60.0));
    assert_eq!(capped.drop, DockDrop::Into { group, index: 2 });
    let column_only = geometry(true, AllowedSplits::Column).classify(Vec2::new(20.0, 60.0));
    assert_eq!(
        column_only.drop,
        DockDrop::Split {
            group,
            side: SplitSide::Top
        },
        "the nearest *offered* edge wins when Left is refused"
    );
    let none = geometry(true, AllowedSplits::None).classify(Vec2::new(20.0, 60.0));
    assert_eq!(none.drop, DockDrop::Into { group, index: 2 });
}

/// The minimal viewer: every tab is a label and an empty body.
#[derive(Debug, Default)]
struct Labels;

impl DockTabs for Labels {
    type Tab = Tab;

    fn title(&mut self, ui: &mut Ui, tab: Tab) -> InternedStr {
        ui.intern(match tab {
            Tab::Main => "main",
            Tab::Prefs => "prefs",
            Tab::Viewer(_) => "viewer",
        })
    }

    fn content(&mut self, ui: &mut Ui, _tab: Tab, _size: Option<Vec2>) {
        Panel::vstack()
            .id_salt("body")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |_| {});
    }

    fn badge(&mut self, tab: Tab) -> TabBadge {
        match tab {
            Tab::Main => TabBadge::Idle,
            _ => TabBadge::None,
        }
    }
}

const SURFACE: UVec2 = UVec2::new(600, 400);

/// Two panes side by side tile the surface: the split gives each half
/// the width, each pane's strip sits along its own top edge, and every
/// chip lands inside the strip that drew it.
#[test]
fn a_split_dock_tiles_its_panes_and_strips() {
    let mut d = seeded();
    let primary = d.primary().id;
    split_off(&mut d, viewer(1), primary, SplitSide::Right);
    let right = d.focused();

    let mut h = UiHarness::new(SURFACE);
    let mut tabs = Labels;
    h.prime(3, |ui| {
        DockView::run(ui, &mut d, &mut tabs);
    });

    let left_pane = h.rect(d.pane_id(primary)).expect("the left pane arranged");
    let right_pane = h.rect(d.pane_id(right)).expect("the right pane arranged");
    assert!(
        (left_pane.size.w - right_pane.size.w).abs() < 2.0,
        "a 0.5 ratio halves the width: {left_pane:?} against {right_pane:?}",
    );
    assert!(
        left_pane.max().x <= right_pane.min.x + 2.0,
        "the panes do not overlap: {left_pane:?} against {right_pane:?}",
    );
    assert_eq!(
        (left_pane.size.h, right_pane.size.h),
        (SURFACE.y as f32, SURFACE.y as f32),
        "both panes take the full height",
    );

    for (group, pane) in [(primary, left_pane), (right, right_pane)] {
        let strip = h.rect(d.strip_id(group)).expect("the strip arranged");
        assert!(
            strip.min.y - pane.min.y < 1.0 && strip.max().y < pane.max().y,
            "the strip rides the pane's top edge: {strip:?} in {pane:?}",
        );
        let content = h
            .layout_rect(d.content_id(group))
            .expect("the content area arranged");
        assert!(
            content.min.y >= strip.max().y - 1.0,
            "the content sits under the strip: {content:?} against {strip:?}",
        );
    }

    // Chip ids are the strip's, keyed on the tab — the same derivation
    // the navigation scan polls.
    let chip = TabStrip::chip_id(d.strip_id(primary), DockState::<Tab>::tab_key(Tab::Prefs));
    let chip_rect = h.rect(chip).expect("the Prefs chip arranged");
    let strip = h.rect(d.strip_id(primary)).expect("the strip arranged");
    assert!(
        strip.contains(chip_rect.center()),
        "the chip sits inside its own strip: {chip_rect:?} in {strip:?}",
    );
}

/// A click on a chip travels through the navigation scan, so the pane it
/// switches to is drawn on the frame the click lands rather than the one
/// after.
#[test]
fn a_chip_click_switches_the_pane_on_the_same_frame() {
    let mut d = seeded();
    let mut h = UiHarness::new(SURFACE);
    let mut tabs = Labels;
    h.prime(3, |ui| DockView::run(ui, &mut d, &mut tabs));
    assert_eq!(d.primary().active_tab(), Tab::Main);

    let chip = TabStrip::chip_id(
        d.strip_id(d.primary().id),
        DockState::<Tab>::tab_key(Tab::Prefs),
    );
    let at = h.center_of(chip);
    h.click_at(at);
    let content = h.frame_value(|ui| {
        DockView::run(ui, &mut d, &mut tabs);
        ui.response_for(d.content_id(d.primary().id)).rect
    });
    assert_eq!(
        d.primary().active_tab(),
        Tab::Prefs,
        "the scan applied the activation before the record"
    );
    assert!(content.is_some(), "the pane kept its content area");
}

/// The close button wins over the activation the same press would
/// otherwise be read as — the button sits inside the chip, so one click
/// reaches both.
#[test]
fn a_close_click_removes_the_tab_and_does_not_activate_it() {
    let mut d = seeded();
    let mut h = UiHarness::new(SURFACE);
    let mut tabs = Labels;
    h.prime(3, |ui| DockView::run(ui, &mut d, &mut tabs));

    let strip = d.strip_id(d.primary().id);
    let close = TabStrip::close_id(strip, DockState::<Tab>::tab_key(Tab::Prefs));
    let at = h.center_of(close);
    h.click_at(at);
    h.frame(|ui| DockView::run(ui, &mut d, &mut tabs));

    assert_eq!(
        d.all_tabs().collect::<Vec<_>>(),
        [Tab::Main, viewer(1)],
        "the close removed the tab"
    );
    assert_ne!(
        d.primary().active_tab(),
        Tab::Prefs,
        "the closed tab is not the active one"
    );
    d.validate().unwrap();
}
