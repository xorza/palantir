# Open issues

- `AnimSpec::spring` accepts springs that oscillate far faster than any frame rate (for example
  stiffness `1e6` with damping `2`, or stiffness `f32::MAX`), and each frame of one lands on an
  arbitrary phase of the swing.
