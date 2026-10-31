# Open issues

- The recovery tests `a_host_recovers_from_a_loss_between_frames` and `a_host_recovers_from_a_second_loss_inside_a_frame` fail at random in `assert_paints_as_fresh`, in both test lanes: about 1 run in 7 to 25 of `cargo drive test recovery`, and 0 in 60 with `--test-threads=1`. The recovered host's first frame differs from a fresh host's in 30 pixels, by 1 or 2 levels, above the top corners of the first two shadowed blocks (x 2..31, y 20..23): the recovered host paints shadow there, the fresh host the ground.
- `cargo drive bench --test` stops at `integration/frame/cpu/hovering`: the frame after a hover
  move between the frame fixture's `enabled` and `submit` probes plans `Skip` where the arm
  expects `Partial` (`src/ui/bench.rs`, `Arm::paints`). It fails at the commit before the harness
  work too.
