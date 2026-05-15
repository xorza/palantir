image

checkbox

skip frame if window is not visible

refactor internals - move them to corresponding modules witf cfg mod

remove
pub(crate) struct DamageEngine { #[cfg(any(test, feature = "internals"))]
pub(crate) dirty: Vec<NodeId>,

move text to framearena

Drain-once for frame_keys/frame_text

3.  CascadesEngine HashMap (2.2% of cascade's 12.9%)

by_id.insert(widget_id, entries.len() as u32); // line 316

by_id is FxHashMap<WidgetId, u32>. Already reused across frames (clear() + reserve(total) at line 207–208 — capacity retained). The 2.2% self is
genuine hash + probe cost for ~500 inserts.

Question worth asking: is by_id actually needed every frame, or only when input dispatch queries by WidgetId? Hit-testing per pointer event is rare
(1–60 events/sec) vs. ~60 record passes/sec. If queries are rare, the map could be built lazily on first query against the current entry_ids vec,
and a "dirty" flag invalidates it when entry_ids changes. That would shave ~2% off the steady-state frame entirely. Worth checking the hit-test call
sites before committing — if the input pass scans by_id per-frame regardless of events, this won't help.
