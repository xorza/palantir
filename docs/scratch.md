image

checkbox

combine wakes that happen almost at same time

PaintMod

skip frame if window is not visible

let input_arrived = self.input.input_arrived_since_last_frame;
self.input.input_arrived_since_last_frame = false;
let wake_fired = fired > 0;
if display_unchanged
&& wake_fired
&& !input_arrived
&& !self.anim.has_pending()
&& !self.relayout_requested
&& self.damage_engine.prev_now.is_some()
&& self
