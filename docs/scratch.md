- showcase agent testing

ScrollRegistry duplicates DeferredRegistry

  - Animation evolution doubles on 2-pass frames (each pass advances). Punting; document as known limitation; fix with snapshot/restore when
  motivated.

  - SeenIds rollover runs in record_phase, so it fires twice on 2-pass frames. Pass A's "current" becomes pass B's "prev" instead of last-painted's
   tree. State eviction works correctly as long as widgets are recorded identically in both passes. For scroll: widget set is identical (only
  reservation values differ) → no state loss. Documented.


  get rid of ui.scrolls

#[allow(dead_code)]
    pub(crate) fn end_frame(&mut self) 

  self.forest.ids.diff_for_sweep();
        let removed = &self.forest.ids.removed; - collaps

both-axes bar shape off by 16px on the corner) are explicitly out of scope for
          this pass — addressable as separate follow-ups.
