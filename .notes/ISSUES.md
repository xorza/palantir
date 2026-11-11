# Open issues

- On an Intel laptop's integrated GPU (Mesa anv, Vulkan), three effect tests miss their
  expected pixels by one 8-bit level: `an_effect_past_its_node_meets_its_ancestors_clip`
  (`[123, 169, 203]` against `[124, 170, 203]`), `a_turn_turns_an_effect_as_a_quad` and
  `an_image_lost_with_its_device_paints_nothing_until_written_again`, on master `f50bf6154` too.
