# Open issues

- A read outside every input scope speaks for the plane's outermost scope, so in an app that
  declares no scope of its own, a focused button's scope is that outermost one: `ui.key_pressed`
  at the app root reads the Enter that clicks the button, and both act on it.
