9. MeasureCache ancestor cache-hit hug-restore coupling
   The cache-hit path in LayoutEngine::measure has to call hugs.restore_subtree(...) for any descendant Grid, or arrange collapses every cell.
   This is a contract between the cache and the Grid driver enforced by a comment + one pinning test. As more drivers accumulate hidden
   per-subtree state (the agent flagged this as the kind of footgun that grows quietly), the cache becomes a graveyard of "remember to restore X
   for driver Y" clauses. Design call: do drivers register their own snapshot/restore hooks against the cache, or
   stays-inline-with-a-comment-per-clause acceptable forever?
