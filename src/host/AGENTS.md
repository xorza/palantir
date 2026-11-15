# Hosts and the harness

`Harness` runs one window with no desktop for unit tests, UI tests and agents; `Remote` plays the
same steps on a live window. The rules of a frame are in `host/harness/mod.rs`'s module doc.

- **The harness is a host**, on the frames the desktop runs, public with no feature. Refused: a
  frame path of its own, which drifts; a separate crate, which cannot reach the core
  ([egui_kittest](https://docs.rs/egui_kittest/latest/egui_kittest/struct.Harness.html),
  [masonry](https://docs.rs/masonry/latest/src/masonry/testing/harness.rs.html),
  [Flutter](https://api.flutter.dev/flutter/flutter_test/WidgetTester-class.html)).
- **Three depths, record by default, on a stepped clock**, so a run repeats and a test pays only
  for what it reads. Refused: a GPU for every test; a CPU rasterizer, a second renderer to keep
  equal to the GPU one.
- **One set of agent verbs on both hosts**, `AgentWindow`, so a driver writes a step once.
- **A harness read mirrors a `Ui` read under its name**, as test sugar: `Harness::rect` and
  `Harness::layout_rect` read a `Response`'s `rect` and `layout_rect`. Refused: a name of the
  harness's own for a read the `Ui` already names, which a reader maps back.
- **A step acts as a person does**: a locator hits what the input reaches, and a step settles,
  spaces presses, refuses to pass with no sign and returns an error an agent reads
  ([Playwright actionability](https://playwright.dev/docs/actionability)). It changes the
  modifiers only when it names some, so a key a person holds stays held.
- **A settle waits for what the window owes now, within a bound**, as Flutter's `pumpAndSettle`.
  Refused: waiting out timers, or a busy app with no limit.
- **No MCP server in palantir**: an async runtime and a fast-moving dependency, or a protocol to
  keep by hand. A second app with a server would argue for a `palantir-mcp` crate.
- **The visual suite stays on `OffscreenHost` with a theme of its own**, as it tests presentation
  paths and no recipe or palette may move a golden.
