# Open issues

- **A cancelled touch on Windows clicks.** winit 0.30.13 reads a pointer message by its `DOWN`, `UP` and `UPDATE` flags only, so an `UP` with `POINTER_FLAG_CANCELED` arrives as `TouchPhase::Ended`, which the host translates as a finger lifted.
- **A touch whose capture moves on Windows never ends.** winit 0.30.13 passes `WM_POINTERCAPTURECHANGED` to `DefWindowProc` and sends no event for it. Windows typically sends no further message for that pointer, so the host holds the finger until focus loss or a start with the same id.
