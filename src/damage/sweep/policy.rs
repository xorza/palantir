//! A policy: the knob values that turn a frame's raw rects into what it repaints.

use crate::damage::Damage;
use crate::damage::region::{DAMAGE_RECT_CAP, DamageRegion, MergePolicy};
use crate::host::window_driver::DIRECT_PROMOTE_COVERAGE;
use crate::primitives::geometry::rect::Rect;
use std::cmp::Ordering;

/// What one frame repaints.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Decision {
    /// A direct whole repaint.
    Full,
    /// These rects through the backbuffer, in a canonical order, so two folds that end alike
    /// compare equal.
    Partial(Vec<Rect>),
}

fn rect_order(a: &Rect, b: &Rect) -> Ordering {
    let key = |r: &Rect| [r.min.y, r.min.x, r.size.h, r.size.w];
    key(a)
        .iter()
        .zip(key(b).iter())
        .map(|(x, y)| x.total_cmp(y))
        .find(|order| order.is_ne())
        .unwrap_or(Ordering::Equal)
}

impl Decision {
    /// A partial repaint of `rects`, none of them lost.
    pub(crate) fn partial(mut rects: Vec<Rect>) -> Self {
        assert!(!rects.is_empty(), "a partial repaint with no rect");
        rects.sort_by(rect_order);
        Self::Partial(rects)
    }

    /// The plan a host paints this by. Its coverage reads zero, as a present path that promoted
    /// on it would take back what the sweep decided.
    pub(crate) fn damage(&self) -> Damage {
        match self {
            Self::Full => Damage::Full,
            Self::Partial(rects) => {
                Damage::Partial(DamageRegion::from_unmerged(rects).unmeasured())
            }
        }
    }
}

/// One setting of the knobs a sweep scores. The full-repaint threshold is not one: on the desktop
/// path the lower promote value already sends every frame past it direct.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Policy {
    pub(crate) merge: MergePolicy,
    /// Coverage past which a frame repaints whole.
    pub(crate) promote_coverage: f32,
}

impl Policy {
    /// What the desktop ships.
    pub(crate) const PRODUCTION: Self = Self {
        merge: MergePolicy::DEFAULT,
        promote_coverage: DIRECT_PROMOTE_COVERAGE,
    };

    /// The budgets a grid sweeps: from strict overlap to one rect, doubling around the shipped
    /// value, as a cost ratio moves by factors.
    const BUDGETS_PX: [f32; 11] = [
        0.0,
        1_000.0,
        2_500.0,
        5_000.0,
        7_000.0,
        10_000.0,
        MergePolicy::DEFAULT.budget_px,
        40_000.0,
        80_000.0,
        160_000.0,
        f32::INFINITY,
    ];

    /// The caps a grid sweeps: one rect, as WebRender; Slint's 3; and the shipped storage.
    const RECT_CAPS: [usize; 5] = [1, 2, 3, 4, DAMAGE_RECT_CAP];

    /// The promote values a grid sweeps.
    const PROMOTES: [f32; 9] = [
        0.1,
        0.2,
        0.3,
        DIRECT_PROMOTE_COVERAGE,
        0.5,
        0.6,
        0.7,
        0.8,
        0.9,
    ];

    /// Every combination of the knob values, [`Self::PRODUCTION`] among them.
    pub(super) fn grid() -> Vec<Self> {
        let mut grid = Vec::with_capacity(
            Self::BUDGETS_PX.len() * Self::RECT_CAPS.len() * Self::PROMOTES.len(),
        );
        for budget_px in Self::BUDGETS_PX {
            for rect_cap in Self::RECT_CAPS {
                for promote_coverage in Self::PROMOTES {
                    grid.push(Self {
                        merge: MergePolicy {
                            budget_px,
                            rect_cap,
                        },
                        promote_coverage,
                    });
                }
            }
        }
        grid
    }

    /// Where [`Self::PRODUCTION`] stands in `grid`.
    pub(super) fn production_idx(grid: &[Self]) -> usize {
        grid.iter()
            .position(|policy| *policy == Self::PRODUCTION)
            .expect("the grid holds production")
    }

    /// What a frame of `raw` rects repaints on `surface`.
    pub(crate) fn decide(&self, raw: &[Rect], surface: Rect) -> Decision {
        let collapsed = DamageRegion::collapse_from(raw, self.merge, surface);
        if collapsed.coverage > self.promote_coverage {
            Decision::Full
        } else {
            Decision::partial(collapsed.region.iter_rects().collect())
        }
    }
}

/// The area `rects` cover together, each pixel once: strips between the sorted x edges, each the
/// merged y spans of the rects across it.
pub(super) fn union_area(rects: &[Rect]) -> f32 {
    let mut xs: Vec<f32> = rects.iter().flat_map(|r| [r.min.x, r.max().x]).collect();
    xs.sort_by(f32::total_cmp);
    xs.dedup();
    let mut spans: Vec<(f32, f32)> = Vec::with_capacity(rects.len());
    let mut area = 0.0f64;
    for edge in xs.windows(2) {
        spans.clear();
        spans.extend(
            rects
                .iter()
                .filter(|r| r.min.x <= edge[0] && r.max().x >= edge[1])
                .map(|r| (r.min.y, r.max().y)),
        );
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut covered = 0.0f64;
        let mut end = f32::NEG_INFINITY;
        for &(from, to) in &spans {
            let from = from.max(end);
            if to > from {
                covered += f64::from(to - from);
                end = to;
            }
        }
        area += covered * f64::from(edge[1] - edge[0]);
    }
    area as f32
}
