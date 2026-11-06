//! A policy: the knob values that turn a frame's raw rects into what it repaints.

use crate::damage::region::{CollapsedDamage, DAMAGE_RECT_CAP, DamageRegion, MergePolicy};
use crate::damage::{Damage, FULL_REPAINT_THRESHOLD};
use crate::host::window_driver::present_facts::{
    PROMOTE_HALF_ROWS_PER_MPX, PROMOTE_PANE_ROWS_PER_MPX, density_share,
};
use crate::primitives::geometry::rect::Rect;
use std::cmp::Ordering;
use std::fmt;

/// What one frame repaints.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Decision {
    /// A direct whole repaint, which leaves the backbuffer stale.
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

/// What a frame's decision reads beside its rects: how much screen, and how much on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameFacts {
    /// The logical window.
    pub(crate) surface: Rect,
    /// The scene's density: the rows that draw on the surface per million physical pixels.
    pub(crate) rows_per_mpx: f32,
    /// The share of the surface pane footprints hold, each pane's counted alone.
    pub(crate) pane_share: f32,
}

/// The coverage past which a frame repaints whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Promote {
    /// One share, whatever the screen holds.
    Coverage(f32),
    /// `d / (d + half)` of the density `d`: the rows that draw per million physical pixels, and
    /// `pane_rows_per_mpx` for all the surface held by pane footprints, as a whole frame captures
    /// and blurs every pane and a partial one only those its change reached.
    Density {
        half_rows_per_mpx: f32,
        pane_rows_per_mpx: f32,
    },
}

impl Promote {
    fn share(self, facts: &FrameFacts) -> f32 {
        match self {
            Self::Coverage(share) => share,
            Self::Density {
                half_rows_per_mpx,
                pane_rows_per_mpx,
            } => density_share(
                facts.rows_per_mpx + pane_rows_per_mpx * facts.pane_share,
                half_rows_per_mpx,
            ),
        }
    }
}

impl fmt::Display for Promote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Coverage(share) => write!(f, "{share:.1}"),
            Self::Density {
                half_rows_per_mpx,
                pane_rows_per_mpx,
            } => write!(f, "d{half_rows_per_mpx:.0}p{pane_rows_per_mpx:.0}"),
        }
    }
}

/// One setting of the knobs a sweep scores.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Policy {
    pub(crate) merge: MergePolicy,
    pub(crate) promote: Promote,
}

impl Policy {
    /// What the desktop ships.
    pub(crate) const PRODUCTION: Self = Self {
        merge: MergePolicy::DEFAULT,
        promote: Promote::Density {
            half_rows_per_mpx: PROMOTE_HALF_ROWS_PER_MPX,
            pane_rows_per_mpx: PROMOTE_PANE_ROWS_PER_MPX,
        },
    };

    /// The budgets a grid sweeps: from strict overlap to one rect, doubling around the shipped
    /// value, as a cost ratio moves by factors.
    const BUDGETS_PX: [f32; 9] = [
        0.0,
        1_000.0,
        2_500.0,
        MergePolicy::DEFAULT.budget_px,
        10_000.0,
        20_000.0,
        40_000.0,
        80_000.0,
        f32::INFINITY,
    ];

    /// The caps a grid sweeps: one rect, as WebRender; and up to the shipped storage.
    const RECT_CAPS: [usize; 4] = [1, 2, 4, DAMAGE_RECT_CAP];

    /// The one shares a grid sweeps.
    const SHARES: [f32; 9] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];

    /// The densities a promote reads one half at, the shipped one among them.
    const HALVES: [f32; 10] = [
        100.0,
        150.0,
        220.0,
        300.0,
        PROMOTE_HALF_ROWS_PER_MPX,
        500.0,
        620.0,
        800.0,
        1000.0,
        1400.0,
    ];

    /// What all of the surface under panes counts for, in rows per Mpx, the shipped value among them.
    const PANE_ROWS: [f32; 7] = [
        0.0,
        100.0,
        200.0,
        PROMOTE_PANE_ROWS_PER_MPX,
        500.0,
        1000.0,
        3000.0,
    ];

    /// Every combination of the knob values, [`Self::PRODUCTION`] among them.
    pub(super) fn grid() -> Vec<Self> {
        let densities = Self::HALVES.into_iter().flat_map(|half_rows_per_mpx| {
            Self::PANE_ROWS
                .into_iter()
                .map(move |pane_rows_per_mpx| Promote::Density {
                    half_rows_per_mpx,
                    pane_rows_per_mpx,
                })
        });
        let promotes: Vec<Promote> = Self::SHARES
            .into_iter()
            .map(Promote::Coverage)
            .chain(densities)
            .collect();
        let mut grid =
            Vec::with_capacity(Self::BUDGETS_PX.len() * Self::RECT_CAPS.len() * promotes.len());
        for budget_px in Self::BUDGETS_PX {
            for rect_cap in Self::RECT_CAPS {
                for &promote in &promotes {
                    grid.push(Self {
                        merge: MergePolicy {
                            budget_px,
                            rect_cap,
                        },
                        promote,
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

    /// What a frame of `raw` rects repaints.
    pub(crate) fn decide(&self, raw: &[Rect], facts: &FrameFacts) -> Decision {
        let collapsed = DamageRegion::collapse_from(raw, self.merge, facts.surface);
        self.decide_collapsed(&collapsed, facts)
    }

    /// [`Self::decide`] over a frame's rects already folded by this policy's merge. Past the
    /// full-repaint threshold a frame is whole before the promote share is read.
    pub(super) fn decide_collapsed(
        &self,
        damage: &CollapsedDamage,
        facts: &FrameFacts,
    ) -> Decision {
        let share = self.promote.share(facts).min(FULL_REPAINT_THRESHOLD);
        if damage.coverage > share {
            Decision::Full
        } else {
            Decision::partial(damage.region.iter_rects().collect())
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
