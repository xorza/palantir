# Frame performance plan: open questions

## M1: a GPU timing for the copy-out

**Item:** M1 in `PERF_PLAN.md`, "a timestamp around the copy-out".

**Why:** on the desktop strategy a partial frame paints its damage into the
backbuffer and then copies the whole backbuffer onto the target, outside
the render pass. `GpuPassStats` times only the inside of the main pass,
so nothing reports what the copy costs apart from the GPU wake-up and the
damage paint. The alternating-arm experiment showed the copy costs about
what a full repaint's resync saves, which is the number this would pin.

**Options:**

1. A new reading beside its neighbours, `GpuPassStats::last_copy_out() ->
   Option<Duration>`, filled from a timestamp pair the backend writes
   around the copy with `CommandEncoder::write_timestamp`. That needs
   `Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`, which `Timing::Instrumented`
   would request where the adapter has it; `None` where it does not. A new
   public method, so it needs your go-ahead.
2. Time the copy as its own render pass with `timestamp_writes`, by doing
   the copy as the blit (`Backbuffer::draw_onto`) when stats are on. No
   new feature, but the timed path then differs from the shipped one
   wherever the target takes a copy.
3. No public reading: measure the copy only in experiment builds, as this
   plan did so far.

**Recommendation:** option 1. It times the path that ships, and it sits
beside `last_pass` and `last_kind` with the same shape.

**Blocked:** only the copy-out timing. No other item depends on it.
