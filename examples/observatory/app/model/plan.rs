//! [`Plan`]: the night's targets, in the order they are imaged.

use crate::app::model::catalog::ObjectId;

/// One target of the night: an object, the filter it is imaged through, and how many exposures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlanEntry {
    pub(crate) object: ObjectId,
    /// An index into [`crate::app::model::session::FILTERS`].
    pub(crate) filter: usize,
    /// The exposures to take, `1..=999`; an `i64`, as a `DragValue` binds one.
    pub(crate) frames: i64,
}

/// The frames a new entry takes.
const DEFAULT_FRAMES: i64 = 20;

/// The night's targets, each object at most once, in imaging order.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Plan {
    entries: Vec<PlanEntry>,
}

impl Plan {
    pub(crate) fn entries(&self) -> &[PlanEntry] {
        &self.entries
    }

    pub(crate) fn entry_mut(&mut self, idx: usize) -> &mut PlanEntry {
        &mut self.entries[idx]
    }

    pub(crate) fn contains(&self, object: ObjectId) -> bool {
        self.entries.iter().any(|entry| entry.object == object)
    }

    /// Append `object` through `filter`; `false`, and no change, when it is in the plan already.
    pub(crate) fn add(&mut self, object: ObjectId, filter: usize) -> bool {
        if self.contains(object) {
            return false;
        }
        self.entries.push(PlanEntry {
            object,
            filter,
            frames: DEFAULT_FRAMES,
        });
        true
    }

    pub(crate) fn remove(&mut self, idx: usize) -> PlanEntry {
        self.entries.remove(idx)
    }

    /// Swap the entry at `idx` with the one before it; its new index, the same at the top.
    pub(crate) fn move_up(&mut self, idx: usize) -> usize {
        if idx == 0 {
            return 0;
        }
        self.entries.swap(idx, idx - 1);
        idx - 1
    }

    /// Swap the entry at `idx` with the one after it; its new index, the same at the bottom.
    pub(crate) fn move_down(&mut self, idx: usize) -> usize {
        if idx + 1 >= self.entries.len() {
            return idx;
        }
        self.entries.swap(idx, idx + 1);
        idx + 1
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Every frame of the plan at `exposure_s` seconds each, in seconds.
    pub(crate) fn integration_s(&self, exposure_s: f64) -> f64 {
        self.entries
            .iter()
            .map(|entry| entry.frames as f64 * exposure_s)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::model::catalog::ObjectId;
    use crate::app::model::plan::Plan;

    fn objects(plan: &Plan) -> Vec<u32> {
        plan.entries().iter().map(|entry| entry.object.0).collect()
    }

    /// Each operation, its edges included: a duplicate add changes nothing, a move past either
    /// end stays, and the integration sums frames × exposure.
    #[test]
    fn every_plan_operation_keeps_its_order() {
        let mut plan = Plan::default();
        assert!(plan.add(ObjectId(7), 0));
        assert!(plan.add(ObjectId(3), 4));
        assert!(plan.add(ObjectId(9), 1));
        assert!(!plan.add(ObjectId(3), 2), "a duplicate");
        assert_eq!(objects(&plan), [7, 3, 9]);
        assert_eq!(
            plan.entries()[1].filter,
            4,
            "the duplicate kept the first filter"
        );

        assert_eq!(plan.move_up(0), 0);
        assert_eq!(plan.move_down(2), 2);
        assert_eq!(plan.move_down(0), 1);
        assert_eq!(objects(&plan), [3, 7, 9]);
        assert_eq!(plan.move_up(2), 1);
        assert_eq!(objects(&plan), [3, 9, 7]);

        plan.entry_mut(0).frames = 10;
        // (10 + 20 + 20) frames × 120 s.
        assert_eq!(plan.integration_s(120.0), 6000.0);

        assert_eq!(plan.remove(1).object, ObjectId(9));
        assert_eq!(objects(&plan), [3, 7]);
        assert!(plan.contains(ObjectId(7)) && !plan.contains(ObjectId(9)));
        plan.clear();
        assert!(plan.entries().is_empty());
        assert_eq!(plan.integration_s(120.0), 0.0);
    }
}
