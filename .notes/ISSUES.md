# Open issues

- A subtree that arrange replays as translated (`LayoutPass::replay_arranged`) is placed at `old rect min + (new slot min − old slot min)`, which can differ from a cold arrange's position by float rounding. In `layout/cache/tests/frames.rs` `cache_rects_match_cold_oracle_across_resizes`, with a Fill hstack of two truncating labels and a Hug vstack of a header above a scroll placed before `canvas-wrap`, step 5 (700 × 450) places the canvas rows at y 600.66144 warm and 600.6615 cold.
