# Open issues

- `Sense`, `KeyFilter`, `PointerWake` and `KeyboardWake` publish the whole
  `bitflags` surface: `from_bits_retain` builds a value with undefined bits,
  `iter()` returns the unnameable `bitflags::iter::Iter`, and the `Internal`,
  `Primitive` and `Bits` associated types leak.
- `InputEvent` and `InputDelta` are public at the crate root, but their only
  public consumer is `internals::UiHarness`, which is feature-gated. Nothing in
  a default build can reach them.
- `WinitHostBuilder::vsync` and `WinitHostBuilder::present_mode` write the same
  slot. The last call wins, and neither doc states the precedence.
- `RgbaU8::hex`, `RgbaU8::BLACK` and `RgbaU8::TRANSPARENT` have no caller in the
  tree. `RgbaU8::hexa`, `RgbaU8::midpoint` and `RgbaU8::is_noop` have had none
  for longer.
