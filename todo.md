transform




3. **`Spacing::all(8.0)` lives in two places** — Button's `with_id` sets it on the element; if Button gains more padding-aware logic later this hardcoding will spread. Consider `ButtonStyle` carrying default padding too.
