# Preferences

How a person's and a system's reductions reach a frame.

- **Reduced motion, transparency and power are preferences on the `Ui`**, each the system's
  answer unless the app picks one, as the color scheme is; all of them, the frame rate among
  them, are one `UiPreferences`, read and written whole. Reduced motion snaps transitions and
  stops decorative motion, an animation its author marks essential running on; reduced
  transparency shows a material over the window's ground with no capture or blur, as WinUI's
  acrylic turns solid ([acrylic](https://learn.microsoft.com/windows/apps/design/style/acrylic)).
  Refused: switches in the `Theme`, a person's choice in a theme file that a transition blends.
- **Reduced power follows the OS power-saver mode**, a mode the person sets, so the system
  decides it. Facts and sources: "Power saving" in
  `.notes/research/2026-10-31-system-preferences.md`. Refused: "on battery" alone, which overrides
  a person who chose no saver mode; a palantir battery threshold, a rule the person never set.
- **Reduced power turns transparency off, reduces the frame rate and snaps transitions**, a pick
  by hand winning; paint loops run on at the reduced rate, as a loop shows what the content is.
  Refused: reduced motion under power, which stops a loop a person needs.
- **A reduced frame rate is at most 30 Hz on whole refresh periods**, input frames included, so
  each frame lands on a vsync, as WebKit in Low Power Mode
  ([WebKit bug 215745](https://bugs.webkit.org/show_bug.cgi?id=215745)); every host holds every
  cause of a frame to it. Refused: decorative motion alone, which saves nothing on an input frame.
- **The frame rate is a preference of its own, which follows reduced power by default**, so an
  app whose motion must stay smooth picks full and keeps the rest of reduced power, as a native
  app picks its rate (`CADisplayLink.preferredFrameRateRange`, `Surface.setFrameRate`). Refused: a
  cap the app cannot lift, as a browser imposes; full power as the only way to lift it.
