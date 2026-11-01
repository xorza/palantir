# Open issues

- A read outside every input scope speaks for the plane's outermost scope, so in an app that
  declares no scope of its own, a focused button's scope is that outermost one: `ui.key_pressed`
  at the app root reads the Enter that clicks the button, and both act on it.
- While a switch's look moves its track border, the knob's top margin reads 2.9998438 px, not the
  3 px inset: the knob subtracts the frame's border, and the track's content box is placed by a
  border 1.6e-4 px apart from it (`widgets/switch/tests.rs`,
  `one_share_places_the_knob_and_blends_its_ink`, 16 ms into the slide).
