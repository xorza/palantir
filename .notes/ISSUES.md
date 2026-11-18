# Open issues

- A spanning cell's intrinsic share (`GridAxis::distribute_span`) holds a Fixed track it crosses at its size in the min-content query too, where the track's own min-content slot is its `min` when it yields, so a span across a yielding Fixed track and a Hug track grows the Hug track short of the span's min-content.
