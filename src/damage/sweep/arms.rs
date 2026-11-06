//! The decisions a scenario can end in, each timed once, and which policy lands on which.

use crate::damage::region::DAMAGE_RECT_CAP;
use crate::damage::region::{CollapsedDamage, DamageRegion, MergePolicy};
use crate::damage::sweep::policy::{Decision, FrameFacts, Policy};
use crate::primitives::geometry::rect::Rect;
use std::fmt::Write;

/// One frame-by-frame way through a scenario's cycle.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Arm {
    pub(super) label: String,
    /// What each frame of the cycle repaints.
    pub(super) frames: Vec<Decision>,
}

/// A scenario's arms, and for each policy of a grid the one it takes.
#[derive(Debug)]
pub(super) struct Arms {
    pub(super) arms: Vec<Arm>,
    /// The arm of each policy of the grid, in the grid's order.
    pub(super) of_policy: Vec<usize>,
}

/// The most rects whose every partition an arm is spent on: 5 for three, 15 for four.
const PARTITIONS_MAX_RECTS: usize = 3;

/// Every partition of `0..count` as each item's block, in restricted growth order: the first item
/// in block 0, each later one in a block up to one past the highest so far.
pub(super) fn partitions(count: usize) -> Vec<Vec<usize>> {
    fn extend(prefix: &mut Vec<usize>, count: usize, out: &mut Vec<Vec<usize>>) {
        if prefix.len() == count {
            out.push(prefix.clone());
            return;
        }
        let blocks = prefix.iter().max().map_or(0, |highest| highest + 1);
        for block in 0..=blocks {
            prefix.push(block);
            extend(prefix, count, out);
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    extend(&mut Vec::with_capacity(count), count, &mut out);
    out
}

/// `raw` clipped to `surface` with each rect once, as a fold would see them.
pub(super) fn visible(raw: &[Rect], surface: Rect) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(raw.len());
    for r in raw {
        let clipped = r.clamp_to(surface);
        if !clipped.is_paint_empty() && !rects.contains(&clipped) {
            rects.push(clipped);
        }
    }
    rects
}

/// `raw`'s rects joined as `partition` says, each block's bounds one rect.
fn joined(raw: &[Rect], partition: &[usize]) -> Decision {
    let blocks = partition.iter().max().map_or(0, |highest| highest + 1);
    let rects = (0..blocks)
        .map(|block| {
            raw.iter()
                .zip(partition)
                .filter(|&(_, &in_block)| in_block == block)
                .map(|(r, _)| *r)
                .reduce(Rect::union)
                .expect("a partition's block holds a rect")
        })
        .collect();
    Decision::partial(rects)
}

/// The name of `decision` over `raw`: `full`, the raw rects each merged rect holds as letters
/// (`AB|C`), or a count where letters would not read.
fn name(decision: &Decision, raw: &[Rect]) -> String {
    let Decision::Partial(rects) = decision else {
        return "full".into();
    };
    if raw.len() > 8 {
        return match rects.len() {
            1 => "1 rect".into(),
            count => format!("{count} rects"),
        };
    }
    let mut blocks: Vec<String> = rects
        .iter()
        .map(|rect| {
            raw.iter()
                .zip('A'..)
                .filter(|(r, _)| rect.contains_rect(**r))
                .map(|(_, letter)| letter)
                .collect()
        })
        .collect();
    blocks.sort();
    blocks.join("|")
}

/// `labels` as one, each run of a repeated label as `label*count`.
fn cycle_label(labels: &[String]) -> String {
    let mut out = String::new();
    let mut at = 0;
    while at < labels.len() {
        let run = labels[at..]
            .iter()
            .take_while(|l| **l == labels[at])
            .count();
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&labels[at]);
        if run > 1 {
            write!(out, "*{run}").expect("a string takes every write");
        }
        at += run;
    }
    out
}

/// `base`, or `base #n` past the `n - 1` arms that hold a name like it: two folds can keep the same
/// count of rects, and a report or a CSV row must tell their arms apart.
fn unique_label(arms: &[Arm], base: String) -> String {
    let taken = |label: &str| arms.iter().any(|arm| arm.label == label);
    if !taken(&base) {
        return base;
    }
    // Past `base`, the arms hold `len - 1` names, so one of these `len` is free.
    (2..=arms.len() + 1)
        .map(|n| format!("{base} #{n}"))
        .find(|label| !taken(label))
        .expect("more names than arms")
}

impl Arms {
    /// The arms of a cycle of `frames` raw rects: where each policy of `policies` lands, the whole
    /// repaint, every frame unmerged where the storage holds it, and for a single frame of a few
    /// rects each partition, so the best arm can be one no policy reaches. Since every policy
    /// lands on an arm, a new knob value needs no new measurement.
    pub(super) fn of(frames: &[Vec<Rect>], facts: &FrameFacts, policies: &[Policy]) -> Self {
        let surface = facts.surface;
        let visibles: Vec<Vec<Rect>> = frames.iter().map(|raw| visible(raw, surface)).collect();
        let mut arms: Vec<Arm> = Vec::new();
        let mut of_policy = Vec::with_capacity(policies.len());
        let mut add = |decisions: Vec<Decision>| -> usize {
            arms.iter()
                .position(|arm| arm.frames == decisions)
                .unwrap_or_else(|| {
                    let labels: Vec<String> = decisions
                        .iter()
                        .zip(&visibles)
                        .map(|(decision, raw)| name(decision, raw))
                        .collect();
                    arms.push(Arm {
                        label: unique_label(&arms, cycle_label(&labels)),
                        frames: decisions,
                    });
                    arms.len() - 1
                })
        };
        // A grid repeats each fold for every other knob, so each folds once.
        let mut folds: Vec<(MergePolicy, Vec<CollapsedDamage>)> = Vec::new();
        for policy in policies {
            let held = folds.iter().position(|(merge, _)| *merge == policy.merge);
            let idx = held.unwrap_or_else(|| {
                let collapsed = frames
                    .iter()
                    .map(|raw| DamageRegion::collapse_from(raw, policy.merge, surface))
                    .collect();
                folds.push((policy.merge, collapsed));
                folds.len() - 1
            });
            of_policy.push(add(folds[idx]
                .1
                .iter()
                .map(|damage| policy.decide_collapsed(damage, facts))
                .collect()));
        }
        add(vec![Decision::Full; frames.len()]);
        if visibles.iter().all(|raw| raw.len() <= DAMAGE_RECT_CAP) {
            add(visibles
                .iter()
                .map(|raw| Decision::partial(raw.clone()))
                .collect());
        }
        if let [raw] = visibles.as_slice()
            && raw.len() <= PARTITIONS_MAX_RECTS
        {
            for partition in partitions(raw.len()) {
                add(vec![joined(raw, &partition)]);
            }
        }
        Self { arms, of_policy }
    }
}
