- showcase agent testing

ScrollRegistry duplicates DeferredRegistry

  - Animation evolution doubles on 2-pass frames (each pass advances). Punting; document as known limitation; fix with snapshot/restore when
  motivated.

  - SeenIds rollover runs in record_phase, so it fires twice on 2-pass frames. Pass A's "current" becomes pass B's "prev" instead of last-painted's
   tree. State eviction works correctly as long as widgets are recorded identically in both passes. For scroll: widget set is identical (only
  reservation values differ) → no state loss. Documented.
