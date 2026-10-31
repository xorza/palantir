# Open issues

- `Ui::with_state` builds a fresh `S::default()` on every call, as `mem::take` leaves one in the state row while the body runs: a state whose `Default` allocates (a `DockState`) allocates every frame.
