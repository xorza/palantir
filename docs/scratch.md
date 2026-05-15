image

checkbox

skip frame if window is not visible

refactor internals - move them to corresponding modules witf cfg mod

remove
pub(crate) struct DamageEngine { #[cfg(any(test, feature = "internals"))]
pub(crate) dirty: Vec<NodeId>,

Drain-once for frame_keys/frame_text

2. Avoid by_id: FxHashMap<WidgetId, u32> rebuild every frame — HashMap::insert is 2.25% of frame. Either: keep last frame's map and
   reconcile diffs, or replace with sorted Vec + binary search (rebuilds cheaper).
3. SeenIds::record 3.44% — that's the collision-detection / record_collision path. Worth a look at whether it could batch the hash check.
