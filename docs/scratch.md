8. Damage granularity for text reshape
   Currently TextShaper reshapes on cache miss regardless of which sub-region damaged. A Damage::Partial(rect) arriving from a text-only change
   still walks the shaper. The question is whether to thread the damage rect into shaping (so unchanged paragraphs short-circuit) or keep
   shape-once-per-key-change. Real perf call, needs a workload.

9. MeasureCache ancestor cache-hit hug-restore coupling
   The cache-hit path in LayoutEngine::measure has to call hugs.restore_subtree(...) for any descendant Grid, or arrange collapses every cell.
   This is a contract between the cache and the Grid driver enforced by a comment + one pinning test. As more drivers accumulate hidden
   per-subtree state (the agent flagged this as the kind of footgun that grows quietly), the cache becomes a graveyard of "remember to restore X
   for driver Y" clauses. Design call: do drivers register their own snapshot/restore hooks against the cache, or
   stays-inline-with-a-comment-per-clause acceptable forever?
