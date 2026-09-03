# Open issues

- `Ui::load_font` schedules no frame. It takes `&self`, so it cannot
  request a repaint, and its doc claims "the frame after one re-shapes the
  text on screen" — which holds only if something else causes a frame. An
  idle UI under `InputPolicy::OnDelta` keeps painting the old faces.

- `text_shape/reuse_layer` charges the shared clock tick to one side of its
  A/B. The `_hit_` arms end a frame and the `_dispatch_` arms do not, so
  the reuse layer pays for a wheel drain a layer-less design would also
  owe. The row sweep belongs to the layer arms alone; the tick does not.
