# Questions from executing `.notes/PERF_REDESIGN.md`

## Q1. Read the last frame by position instead of by id

**Item:** C4, the per-widget lookups of last frame's data.

**Background.** Each widget's id reaches several independent hash maps in
every frame. After C1 (`SeenIds` in step with the last frame), the ones left
on `cached_cpu` are: `Ui::response_for` probing `Cascade::by_id` (3.2%),
`AnimMap::animate` (1.8%), `Cascade::is_within` (1.0%, two probes per call),
and the state map for stateful widgets. While a pass is in step, a widget's
entry in `SeenIds` has the same position as last frame's entry for the same
id, and that entry already holds last frame's endpoint. So these reads could
take last frame's row by position, and probe only out of step.

**Why it needs a decision.** It reaches the cascade, input and animation
subsystems, not only the id tracker. A two-pass frame needs care: during
pass B, `Cascade::by_id` holds pass A's rows, while `SeenIds::prev` holds the
last painted frame's, so a positional read must name which snapshot it
reads.

**Options.**

1. **Positional reads with a probe fallback.** `ResolvedId` carries the
   entry position. `response_for` and the look animation read last frame's
   row through it while in step and the cascade snapshot belongs to that
   frame, and probe otherwise. Estimated gain: 3–5% of `cached_cpu`.
2. **One slot per widget.** A stable dense slot index, kept across frames
   like a generational arena, that every per-widget store (cascade row,
   animation, state) indexes instead of hashing the id. The largest gain and
   the largest change.
3. **Leave the lookups as they are.**

**Recommendation:** option 1, after C2. It keeps every store as it is and
only adds a faster path, as C1 did.

**Blocked:** the positional part of C4. The other C4 items go on.
