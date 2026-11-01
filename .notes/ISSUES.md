# Open issues

- `host::winit::remote::tests::a_remote_plays_and_reads_a_live_window` fails now and then on the
  macOS CI runner at `src/host/winit/remote/tests.rs:209`: the query made after
  `run_on_main` posts a 100 ms sleep returns something other than `PlayError::NoFrame` (run
  41328476183; the same tree passed in run 41326714379).
