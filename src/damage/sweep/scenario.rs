//! The damage a sweep measures: what real screens change, as rects on a reference window.

use crate::primitives::geometry::rect::Rect;
use glam::Vec2;

/// The window every scenario's rects lie on; a stage of another size scales them, so a rect keeps
/// its share of the surface.
pub(super) const REFERENCE: Vec2 = Vec2::new(1280.0, 720.0);

/// Which stages a scenario runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Every stage: what a change of content, scale or surface size is read against.
    Core,
    /// The stages at the bench surface with a whole screen on them.
    Primary,
    /// The stage whose screen the rects were recorded on, the only one they describe.
    Captured,
}

/// One cycle of frames' raw damage.
#[derive(Clone, Debug)]
pub(crate) struct Scenario {
    pub(crate) name: String,
    /// Each frame's raw rects, unmerged, in the order a fold takes them.
    pub(crate) frames: Vec<Vec<Rect>>,
    /// Frames this stands for in a weighted mean: one for a made case, the count for a recorded one.
    pub(crate) weight: u32,
    pub(crate) scope: Scope,
}

impl Scenario {
    fn new(name: impl Into<String>, scope: Scope, frames: Vec<Vec<Rect>>) -> Self {
        Self {
            name: name.into(),
            frames,
            weight: 1,
            scope,
        }
    }

    fn still(name: impl Into<String>, scope: Scope, rects: &[Rect]) -> Self {
        Self::new(name, scope, vec![rects.to_vec()])
    }
}

/// A rect of `coverage` of the reference window, centered at its aspect.
fn block(coverage: f32) -> Rect {
    let side = coverage.sqrt();
    let size = REFERENCE * side;
    let at = (REFERENCE - size) / 2.0;
    Rect::new(at.x, at.y, size.x, size.y)
}

/// A band across the whole width, of `coverage` of the window.
fn wide(coverage: f32) -> Rect {
    let height = REFERENCE.y * coverage;
    Rect::new(0.0, (REFERENCE.y - height) / 2.0, REFERENCE.x, height)
}

/// A band over the whole height, of `coverage` of the window.
fn tall(coverage: f32) -> Rect {
    let width = REFERENCE.x * coverage;
    Rect::new((REFERENCE.x - width) / 2.0, 0.0, width, REFERENCE.y)
}

/// `count` spinners of 24 px, evenly over the window.
fn spinners(count: u32) -> Vec<Rect> {
    let cols = count.isqrt() + u32::from(count.isqrt().pow(2) < count);
    let rows = count.div_ceil(cols);
    (0..count)
        .map(|i| {
            let cell = Vec2::new(
                REFERENCE.x / cols as f32 * ((i % cols) as f32 + 0.5),
                REFERENCE.y / rows as f32 * ((i / cols) as f32 + 0.5),
            );
            Rect::new(cell.x - 12.0, cell.y - 12.0, 24.0, 24.0)
        })
        .collect()
}

/// The linear congruential sequence `ranqd1` of Numerical Recipes, so every run and host draws
/// alike.
#[derive(Debug)]
pub(super) struct Ranqd1(u32);

impl Ranqd1 {
    pub(super) const fn new(seed: u32) -> Self {
        Self(seed)
    }

    /// The next 24 bits, the sequence's high ones: its low bits barely vary.
    pub(super) const fn next_bits(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }
}

/// `count` rects, 8 to 120 px wide and 8 to 60 px high, scattered over the window.
fn scatter(count: u32) -> Vec<Rect> {
    let mut sequence = Ranqd1::new(0x2545_F491);
    let mut next = |bound: f32| sequence.next_bits() as f32 / (1u32 << 24) as f32 * bound;
    (0..count)
        .map(|_| {
            let (w, h) = (8.0 + next(112.0), 8.0 + next(52.0));
            Rect::new(next(REFERENCE.x - w), next(REFERENCE.y - h), w, h)
        })
        .collect()
}

/// Two rows of 300 × 60 bricks, 1 px apart, the second shifted half a brick: seven in all, within
/// the cap. A pair in a row wastes 60 px² in a fold, and split, its scissors' padding overlaps.
fn bricks() -> Vec<Rect> {
    let row = |y: f32, shift: f32, count: u32| {
        (0..count).map(move |col| Rect::new(shift + col as f32 * 301.0, y, 300.0, 60.0))
    };
    row(200.0, 0.0, 4).chain(row(261.0, 150.0, 3)).collect()
}

/// The sides of a `width` px outline of `outline`.
fn ring(outline: Rect, width: f32) -> Vec<Rect> {
    let (at, size) = (outline.min, outline.size);
    vec![
        Rect::new(at.x, at.y, size.w, width),
        Rect::new(at.x, at.y + size.h - width, size.w, width),
        Rect::new(at.x, at.y + width, width, size.h - 2.0 * width),
        Rect::new(
            at.x + size.w - width,
            at.y + width,
            width,
            size.h - 2.0 * width,
        ),
    ]
}

/// A card sliding 16 px a frame: each frame damages where it was and where it is.
fn sliding_card() -> Vec<Vec<Rect>> {
    let card = |step: u32| Rect::new(300.0 + step as f32 * 16.0, 200.0, 240.0, 160.0);
    (0..8)
        .map(|step| vec![card(step), card((step + 1) % 8)])
        .collect()
}

/// A menu opening from the corner of its trigger, up to 60 % of the window.
fn growing_menu() -> Vec<Vec<Rect>> {
    (1..=8)
        .map(|step| {
            let share = (0.6 * step as f32 / 8.0).sqrt();
            vec![Rect::new(
                0.0,
                0.0,
                REFERENCE.x * share,
                REFERENCE.y * share,
            )]
        })
        .collect()
}

const CARET: Rect = Rect::new(100.0, 80.0, 2.0, 18.0);
const CLOCK: Rect = Rect::new(1200.0, 696.0, 60.0, 18.0);
const SPINNER: Rect = Rect::new(640.0, 340.0, 24.0, 24.0);
const SIDEBAR: Rect = Rect::new(0.0, 0.0, 240.0, 720.0);
const TOOLBAR: Rect = Rect::new(240.0, 0.0, 1040.0, 48.0);
/// The same toolbar reaching 1 px over the sidebar.
const TOOLBAR_OVER: Rect = Rect::new(239.0, 0.0, 1041.0, 48.0);
const STATUS_BAR: Rect = Rect::new(0.0, 692.0, 1280.0, 28.0);
const INSPECTOR: Rect = Rect::new(1000.0, 0.0, 280.0, 720.0);

/// The coverages every one-rect shape runs at, none on a promote value of the policy grid, so a
/// rounding cannot pick a side.
const ONE_RECT_PCTS: [u32; 8] = [1, 5, 15, 25, 35, 45, 55, 75];

fn single_rects(out: &mut Vec<Scenario>) {
    for pct in ONE_RECT_PCTS {
        let coverage = pct as f32 / 100.0;
        out.push(Scenario::still(
            format!("one/block_{pct:02}"),
            Scope::Core,
            &[block(coverage)],
        ));
        out.push(Scenario::still(
            format!("one/wide_{pct:02}"),
            Scope::Primary,
            &[wide(coverage)],
        ));
        out.push(Scenario::still(
            format!("one/tall_{pct:02}"),
            Scope::Primary,
            &[tall(coverage)],
        ));
    }
}

fn pairs(out: &mut Vec<Scenario>) {
    let row = |y: f32| Rect::new(40.0, y, 400.0, 32.0);
    let tab = |x: f32| Rect::new(x, 8.0, 120.0, 32.0);
    let icon = |x: f32, y: f32| Rect::new(x, y, 44.0, 44.0);
    let card = |x: f32| Rect::new(x, 200.0, 240.0, 160.0);
    let cases: Vec<(&str, Scope, Vec<Rect>)> = vec![
        // Hover moves between neighbors: list rows and tabs.
        (
            "pair/rows_gap0",
            Scope::Primary,
            vec![row(100.0), row(132.0)],
        ),
        ("pair/rows_gap1", Scope::Core, vec![row(100.0), row(133.0)]),
        (
            "pair/rows_gap8",
            Scope::Primary,
            vec![row(100.0), row(140.0)],
        ),
        (
            "pair/tabs_gap0",
            Scope::Primary,
            vec![tab(100.0), tab(220.0)],
        ),
        (
            "pair/tabs_gap8",
            Scope::Primary,
            vec![tab(100.0), tab(228.0)],
        ),
        // Status icons on one row or one column.
        (
            "pair/icons_gap64",
            Scope::Primary,
            vec![icon(20.0, 8.0), icon(128.0, 8.0)],
        ),
        (
            "pair/icons_gap400",
            Scope::Core,
            vec![icon(20.0, 8.0), icon(464.0, 8.0)],
        ),
        (
            "pair/icons_gap1000",
            Scope::Primary,
            vec![icon(20.0, 8.0), icon(1064.0, 8.0)],
        ),
        (
            "pair/icons_col_gap64",
            Scope::Primary,
            vec![icon(8.0, 20.0), icon(8.0, 128.0)],
        ),
        (
            "pair/icons_col_gap400",
            Scope::Primary,
            vec![icon(8.0, 20.0), icon(8.0, 464.0)],
        ),
        // Small and far on a diagonal: a caret and a clock.
        ("pair/caret_clock", Scope::Primary, vec![CARET, CLOCK]),
        (
            "pair/corners",
            Scope::Primary,
            vec![
                Rect::new(0.0, 0.0, 16.0, 16.0),
                Rect::new(1264.0, 704.0, 16.0, 16.0),
            ],
        ),
        // One far larger than the other: a tooltip, a menu, a panel with a caret far off.
        (
            "pair/tooltip_button",
            Scope::Primary,
            vec![
                Rect::new(300.0, 200.0, 200.0, 28.0),
                Rect::new(290.0, 232.0, 120.0, 32.0),
            ],
        ),
        (
            "pair/menu_trigger",
            Scope::Primary,
            vec![
                Rect::new(200.0, 60.0, 240.0, 320.0),
                Rect::new(200.0, 32.0, 80.0, 28.0),
            ],
        ),
        (
            "pair/panel30_caret",
            Scope::Primary,
            vec![
                Rect::new(0.0, 0.0, 640.0, 432.0),
                Rect::new(1200.0, 700.0, 2.0, 18.0),
            ],
        ),
        // Two strips meeting at a corner: 24 % touching, the whole window at 1 px of overlap.
        ("pair/l_touching", Scope::Core, vec![SIDEBAR, TOOLBAR]),
        (
            "pair/l_overlap",
            Scope::Primary,
            vec![SIDEBAR, TOOLBAR_OVER],
        ),
        // Strips that cross: a crosshair, a splitter.
        (
            "pair/cross_strips",
            Scope::Core,
            vec![
                Rect::new(630.0, 0.0, 20.0, 720.0),
                Rect::new(0.0, 350.0, 1280.0, 20.0),
            ],
        ),
        (
            "pair/cross_thin",
            Scope::Primary,
            vec![
                Rect::new(640.0, 0.0, 1.0, 720.0),
                Rect::new(0.0, 360.0, 1280.0, 1.0),
            ],
        ),
        // Bars on opposite edges, whose bounds are the whole window.
        (
            "pair/bars_top_bottom",
            Scope::Primary,
            vec![Rect::new(0.0, 0.0, 1280.0, 48.0), STATUS_BAR],
        ),
        (
            "pair/bars_left_right",
            Scope::Primary,
            vec![SIDEBAR, INSPECTOR],
        ),
        // A dragged card's old and new place, and two windows sharing a corner.
        (
            "pair/drag_overlap90",
            Scope::Primary,
            vec![card(300.0), card(324.0)],
        ),
        (
            "pair/drag_overlap50",
            Scope::Primary,
            vec![card(300.0), card(420.0)],
        ),
        (
            "pair/drag_overlap5",
            Scope::Primary,
            vec![card(300.0), card(528.0)],
        ),
        (
            "pair/windows_corner",
            Scope::Primary,
            vec![
                Rect::new(100.0, 100.0, 400.0, 300.0),
                Rect::new(480.0, 380.0, 400.0, 300.0),
            ],
        ),
        // A card and a button inside it.
        (
            "pair/contained",
            Scope::Primary,
            vec![
                Rect::new(100.0, 100.0, 300.0, 200.0),
                Rect::new(200.0, 200.0, 80.0, 24.0),
            ],
        ),
        // Two panels in opposite corners: their sum on each side of 40 %, their bounds whole.
        (
            "pair/panels_sum38",
            Scope::Primary,
            vec![
                Rect::new(0.0, 0.0, 400.0, 437.0),
                Rect::new(880.0, 283.0, 400.0, 437.0),
            ],
        ),
        (
            "pair/panels_sum42",
            Scope::Primary,
            vec![
                Rect::new(0.0, 0.0, 400.0, 484.0),
                Rect::new(880.0, 236.0, 400.0, 484.0),
            ],
        ),
    ];
    out.extend(
        cases
            .into_iter()
            .map(|(name, scope, rects)| Scenario::still(name, scope, &rects)),
    );
}

fn triples(out: &mut Vec<Scenario>) {
    let cases: [(&str, Scope, [Rect; 3]); 7] = [
        (
            "triple/tabs",
            Scope::Primary,
            [
                Rect::new(100.0, 8.0, 120.0, 32.0),
                Rect::new(224.0, 8.0, 120.0, 32.0),
                Rect::new(348.0, 8.0, 120.0, 32.0),
            ],
        ),
        (
            "triple/cluster_outlier",
            Scope::Core,
            [
                Rect::new(20.0, 8.0, 44.0, 44.0),
                Rect::new(72.0, 8.0, 44.0, 44.0),
                CLOCK,
            ],
        ),
        (
            "triple/scroll_counter",
            Scope::Primary,
            [
                Rect::new(1268.0, 40.0, 12.0, 600.0),
                Rect::new(268.0, 40.0, 1000.0, 600.0),
                CLOCK,
            ],
        ),
        (
            "triple/l_caret",
            Scope::Primary,
            [SIDEBAR, TOOLBAR, Rect::new(700.0, 400.0, 2.0, 18.0)],
        ),
        ("triple/scattered", Scope::Primary, [CARET, SPINNER, CLOCK]),
        (
            "triple/staircase",
            Scope::Primary,
            [
                Rect::new(200.0, 100.0, 100.0, 100.0),
                Rect::new(310.0, 210.0, 100.0, 100.0),
                Rect::new(420.0, 320.0, 100.0, 100.0),
            ],
        ),
        (
            "triple/stripes",
            Scope::Primary,
            [
                Rect::new(200.0, 100.0, 600.0, 24.0),
                Rect::new(200.0, 140.0, 600.0, 24.0),
                Rect::new(200.0, 180.0, 600.0, 24.0),
            ],
        ),
    ];
    out.extend(
        cases
            .iter()
            .map(|(name, scope, rects)| Scenario::still(*name, *scope, rects)),
    );
}

fn many_rects(out: &mut Vec<Scenario>) {
    let list = (0..32)
        .map(|i| Rect::new(40.0, 10.0 + i as f32 * 22.0, 400.0, 20.0))
        .collect::<Vec<_>>();
    let cases: Vec<(&str, Scope, Vec<Rect>)> = vec![
        // A marquee's outline, thin and thick: little area, bounds of most of the window.
        (
            "many/ring_thin",
            Scope::Primary,
            ring(Rect::new(300.0, 150.0, 600.0, 400.0), 2.0),
        ),
        (
            "many/ring_thick",
            Scope::Primary,
            ring(Rect::new(300.0, 150.0, 600.0, 400.0), 16.0),
        ),
        // Spinners across the window, around the fold's cap of 8 and past it.
        ("many/spinners_08", Scope::Primary, spinners(8)),
        ("many/spinners_09", Scope::Primary, spinners(9)),
        ("many/spinners_16", Scope::Core, spinners(16)),
        ("many/spinners_64", Scope::Primary, spinners(64)),
        // A list refreshing row by row, whose rows share one bounds.
        ("many/rows_32", Scope::Primary, list),
        ("many/scatter_64", Scope::Primary, scatter(64)),
        ("many/bricks_gap1", Scope::Primary, bricks()),
    ];
    out.extend(
        cases
            .into_iter()
            .map(|(name, scope, rects)| Scenario::still(name, scope, &rects)),
    );
}

/// Cycles whose frames differ: a frame's cost then depends on what the last one left, as a direct
/// frame leaves the backbuffer stale.
fn sequences(out: &mut Vec<Scenario>) {
    let spike: Vec<Vec<Rect>> = (0..16)
        .map(|i| {
            if i == 15 {
                vec![Rect::new(0.0, 0.0, REFERENCE.x, REFERENCE.y)]
            } else {
                vec![CARET]
            }
        })
        .collect();
    out.push(Scenario::new(
        "seq/oscillate_39_41",
        Scope::Core,
        vec![vec![block(0.39)], vec![block(0.41)]],
    ));
    out.push(Scenario::new(
        "seq/alternate_caret_60",
        Scope::Primary,
        vec![vec![CARET], vec![block(0.6)]],
    ));
    out.push(Scenario::new("seq/spike_1_in_16", Scope::Core, spike));
    out.push(Scenario::new(
        "seq/sliding_card",
        Scope::Primary,
        sliding_card(),
    ));
    out.push(Scenario::new(
        "seq/growing_menu",
        Scope::Primary,
        growing_menu(),
    ));
}

/// Every made scenario a stage may measure, in the order the report lists them.
pub(crate) fn catalog() -> Vec<Scenario> {
    let mut out = Vec::new();
    single_rects(&mut out);
    pairs(&mut out);
    triples(&mut out);
    many_rects(&mut out);
    sequences(&mut out);
    out
}

/// The scenario called `name`, for a case that names one.
pub(crate) fn named(name: &str) -> Scenario {
    catalog()
        .into_iter()
        .find(|scenario| scenario.name == name)
        .unwrap_or_else(|| panic!("no scenario {name}"))
}

/// The pairs of the full product of size ratio, aspect and relation, around a 200 × 200 rect.
/// Its fold and decision read in milliseconds with no device, so nothing times them.
pub(super) fn pair_grid() -> Vec<Scenario> {
    const A: Rect = Rect::new(540.0, 260.0, 200.0, 200.0);
    let a_area = A.area();
    let a_max = A.max();
    let mut out = Vec::new();
    for (ratio_label, ratio) in [("r1", 1.0f32), ("r10", 0.1), ("r100", 0.01)] {
        for (aspect_label, aspect) in [
            ("square", 1.0f32),
            ("wide", 8.0),
            ("tall", 0.125),
            ("thin", 32.0),
        ] {
            let height = (a_area * ratio / aspect).sqrt();
            let width = height * aspect;
            let beside = |x: f32, y: f32| Rect::new(x, y, width, height);
            let middle_y = A.min.y + (A.size.h - height) / 2.0;
            let inside = width <= A.size.w && height <= A.size.h;
            let contained = inside.then(|| beside(A.min.x + (A.size.w - width) / 2.0, middle_y));
            let relations = [
                ("contained", contained),
                ("overlap", Some(beside(a_max.x - width / 4.0, middle_y))),
                ("touching", Some(beside(a_max.x, middle_y))),
                ("corner", Some(beside(a_max.x, a_max.y))),
                ("near", Some(beside(a_max.x + 8.0, middle_y))),
                ("far", Some(beside(a_max.x + 300.0, middle_y))),
                ("diagonal", Some(beside(a_max.x + 200.0, a_max.y + 100.0))),
            ];
            for (relation, b) in relations {
                if let Some(b) = b {
                    out.push(Scenario::still(
                        format!("grid/{ratio_label}/{aspect_label}/{relation}"),
                        Scope::Primary,
                        &[A, b],
                    ));
                }
            }
        }
    }
    out
}
