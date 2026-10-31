# A `Stencil8` stencil on reused memory loses 4×4 blocks (2026-10-31)

All measurements on an RTX 4090 Laptop GPU, Vulkan, NVIDIA driver 615.78.08, wgpu 30.0.1,
gpu-allocator 0.28.0, palantir at `d92e5df6a`. The test code the measurements ran, a palantir
bisect test, a stencil read-back patch and a wgpu-only program, was not kept; the parts below
describe what each did.

## The cause

- **An `S8_UINT` image bound to memory that held nonzero data keeps nonzero 4×4 blocks after
  `LOAD_OP_CLEAR`.** wgpu maps `Stencil8` to `VK_FORMAT_S8_UINT` on this GPU. A new stencil that
  gpu-allocator places where freed buffers held nonzero bytes reads 128, 130 or 132 in some
  4 × 4 blocks after a render pass whose only work is a stencil clear to 0 and a store. A second
  clear pass removes the 128s and leaves the 130s and 132s.
- **The values do not depend on the old data.** Buffers filled with any nonzero byte give the same
  three values; buffers filled with 0, or never written, give none. One value for each 4 × 4
  block reads as the driver's per-block state, not a copy of the old bytes.
- **Only `S8_UINT`.** `Depth24PlusStencil8` and `Depth32FloatStencil8` on the same reused memory
  show nothing.
- **Shown with wgpu alone, on one thread** (the wgpu-only program, `mix` mode), so palantir's
  code is not the cause. Vulkan leaves the contents of reused memory undefined, and a load clear
  must define every texel of the attachment; this one does not. It reads as a driver fault.
  wgpu's own part, the `UNDEFINED` layout barrier before the pass, was not checked against a
  raw-Vulkan program.
- **Why palantir sees lost blocks.** A frame with a rounded clip gives its main pass a stencil
  attachment, and every draw in that pass tests the stencil `Equal` 0 outside a mask. Where a
  block holds 130, the draws fail and the clear color stays.
- **Why it looked like a threading bug.** Threads only change where gpu-allocator puts each new
  stencil. One thread that keeps several hosts alive and frees them in a random order shows it at
  the same rate; one thread that makes and frees one host at a time puts each stencil back on the
  memory the last stencil held, and never on a buffer's.

## The symptom

- **What a wrong frame shows.** Blocks of 4 × 4 pixels, aligned to multiples of 4, keep the
  frame's clear color where a shape is drawn. Any kind of shape loses them: a solid rect, a
  gradient, a polyline, a shadow's corner. One to four blocks a frame (16 to 64 pixels).
- **It stays.** In the host's next frame, a partial repaint whose damage covers the block, the
  block is often missing again.
- **It is in the texture.** A second read-back of a wrong frame, after `Gpu::wait`, returns the
  same pixels.
- **Where it showed first.** `assert_paints_as_fresh` in `src/host/offscreen/tests/recovery.rs`
  failed about 1 run in 7 to 25 of `cargo drive test recovery`. In one failure a third new host
  painted what the recovered host painted: the odd frame was the comparison host's. The recovery
  is not the cause.

## Measurements in palantir

First frames of new hosts on `headless_test_gpu`. "Wrong" compares colors with a reference
frame; "dirty" counts frames whose stencil, read back after the frame, holds a nonzero texel
outside the clip's square, which finds three times as many frames, as most spoiled blocks lie
under no shape.

| Setup | Result |
|---|---|
| Recovery scene, 8 threads, one shared device | 4 / 2000 wrong |
| Recovery scene, 8 threads, a device for each thread | 0 / 2000 wrong |
| Recovery scene, 1 thread, one host at a time | 0 / 3840 wrong |
| Recovery tests, each on an isolated device, 60 runs | 0 failed runs |

The recovery scene with one part taken out at a time, 8 threads, 4000 frames, two lanes:

| Variant | Wrong, lane 1 / 2 |
|---|---|
| All | 11 / 5 |
| No rounded clip | **0 / 0** |
| No mesh / no line / no image | 15 / 18, 14 / 4, 6 / 10 |
| Clip with rects, gradient, shadows, text, polyline | 24 / 17 |

The clip with one shape kind, 2000 frames, `WGPU_VALIDATION=0`:

| Variant | Dirty, lane 1 / 2 |
|---|---|
| Clip alone | 0 / 0 (4000 frames) |
| Clip and solid rect, gradient, text or polyline | 0 / 0 each |
| Clip and four shadows | 52 / 54, a second run 60 / 69 |
| Clip and four shadows, cutout tables off | 56 / 53 |

The shadows change what the host allocates; they are not needed in the wgpu-only program.

The clip-and-shadows scene, changed one way at a time, 2000 frames, two lanes:

| Change | Dirty, lane 1 / 2 |
|---|---|
| None | 43 / 45 |
| Each frame's GPU work serialized under one lock, waited before the next | 64 / 67 |
| Hosts kept alive in rounds of 32 a thread | 1 / 6 |
| Every stencil texture kept alive, never freed | **0 / 0** (same run time) |
| One thread, 8 hosts alive, a pseudo-random one replaced each frame | 43 / 33 |
| Stencil `StoreOp::Store` instead of `Discard` (full scene, 8000) | 25 wrong |
| Stencil `Depth24PlusStencil8` instead of `Stencil8` (full scene, 8000) | **0 / 0** wrong |
| `WGPU_DEBUG=0`, no Vulkan object names (8000) | 30 / 30 wrong |
| `WGPU_VALIDATION=0`, no Khronos layer (8000) | 65 wrong |

- **Not concurrent GPU work.** Serializing the frames does not lower the rate.
- **Where the stencil lands.** From gpu-allocator's report (`Device::generate_allocator_report`)
  in the one-thread run: of the new stencils whose memory an earlier stencil held at the same
  start, 0 of 193 were dirty; at another start, 45 of 1041; on memory no stencil held in the
  last 8 frames, 10 of 278. Dirty stencils sat on memory that buffers (`*.vbuf`, `*.instances`,
  `quad.masks`, `cutouts.corners`) had held; none on memory a backbuffer or color target had held.
- **Before any draw.** With an extra pass that only clears the stencil and stores it, run before
  the frame's passes, 12 to 77 frames out of 2000 already held the values after that pass.
- **Not the object names.** Dawn removed its Vulkan debug labels for an NVIDIA driver memory
  corruption (<https://dawn.googlesource.com/dawn/+/4e5bb20e628b6fc1052b5a65f939df4b87e7b823%5E>);
  here the rate does not move without them.
- **Not the validation layer.** Version 1.4.357 had a heap corruption under concurrent render pass
  creation (<https://github.com/KhronosGroup/Vulkan-ValidationLayers/issues/13535>); without the
  layer the rate rises, with the faster frames. Its synchronization validation reported no hazard.

## wgpu alone

The wgpu-only program uses no palantir code. `<threads> <steps> mix <slots> <kinds>` keeps
`slots` hosts alive and replaces a pseudo-random one each step. A new host makes 3 to 6 buffers of
256 to 16 640 bytes (`VERTEX | COPY_DST`), each filled with one random byte by
`queue.write_buffer`, and a 160 × 120 stencil, in a random order; the stencil is cleared by an
empty pass, stored, copied and read back. Letters in `kinds`: `c` a color target too, `q` no
stamped pass after the check, `z` buffers filled with 0, `n` buffers never written, `d`
`Depth24PlusStencil8`, `3` `Depth32FloatStencil8`, `2` two clear passes.

| `threads steps mix slots kinds` | Stencils nonzero after the clear |
|---|---|
| `1 4000 mix 8 b` | 38, 42 |
| `1 4000 mix 1 b` | 29 to 56 |
| `1 4000 mix 1 bq` (no draw at all) | 56, 57 |
| `1 4000 mix 1 cbq` | 64 |
| `1 4000 mix 8 x` (no buffers) | 0 |
| `1 4000 mix 1 bzq` (buffers of 0) | 0 |
| `1 4000 mix 1 bnq` (buffers never written) | 0 |
| `1 4000 mix 1 bqd` (`Depth24PlusStencil8`) | 0 |
| `1 4000 mix 1 bq3` (`Depth32FloatStencil8`) | 0 |
| `1 4000 mix 1 bq2` (two clears) | 48, values 130 and 132 only |

The values printed are always 128, 130 and 132. The earlier modes of the program found nothing,
as their stencils never landed on a written buffer's memory: `stamp`, `stamp churn`, `jitter`,
`pause=`, `reuse` and `shift` (a stencil placed 0 to 9216 bytes below a freed stamped stencil,
checked through gpu-allocator's report, 0 dirty in 3200).

## What clears the blocks with `Stencil8` kept

The wgpu-only program, `mix` mode, one thread, more letters in `kinds`: `r` a pass that draws a
frame-wide quad with `Always`/`Replace` 0 and write mask `0xff` after the clear, `w` zeros copied
into the stencil aspect from a zeroed buffer (`copy_buffer_to_texture`) before the first pass,
`v` the same zeros through `queue.write_texture`, `k` three more clear passes after.

| `kinds` | Stencils nonzero, of 4000 or 8000 |
|---|---|
| `bq` | 57 / 4000 |
| `bqk` (four clears) | 45 / 4000, values 130 and 132 |
| `bqr` (a draw writes 0 to every texel) | 49 / 4000, values 130 and 132 |
| `bqrk` | 50 / 4000 |
| `bqw` (zeros copied in) | **0 / 4000** |
| `bqwk` | **0 / 4000** |
| `bqv`, `bqvk` (zeros written through the queue) | **0 / 8000** each |
| `bw`, slots 1 and 8; `cbw`, slots 8 (with the stamped pass) | **0 / 8000** each |
| `cb`, slots 8 (no zeros) | 80 / 8000 |

- **The render path cannot clear the blocks; a transfer write can.** Neither a load clear nor a
  draw that writes 0 through the stencil test changes a spoiled block. A copy or queue write of
  zeros into the stencil aspect, once, before the first pass, leaves no spoiled block, and later
  clears keep it clean. wgpu's `clear_texture` clears a depth or stencil texture through render
  passes (`wgpu-core` `command/clear.rs`, `clear_texture_via_render_passes`), so it is no remedy.
- **In palantir.** `Stencil::new` writing zeros through `queue.write_texture` (`COPY_DST` added to
  the stencil's usage): the clip-and-shadows scene 0 of 2000 in each lane with one thread and 8
  hosts (43 / 33 without), 0 of 2000 in each lane with 8 threads (43 / 45 without); the full
  recovery scene 0 of 8000 in each lane with 8 threads (11 / 5 wrong of 4000 without).
- **The unit test.** `gpu::surface::stencil::tests::a_stencil_on_memory_buffers_held_clears_to_zero`
  (600 rounds, 0.6 s) passes with the write and fails at round 249 in both lanes without it.

## Not done

- A raw-Vulkan program (an `S8_UINT` image bound over a written buffer's memory, a clear-only
  render pass, a copy out), to show the fault without wgpu and to report it to NVIDIA. It needs
  `ash` as a dependency of that program.
- Other GPUs and drivers: lavapipe would not open a device here ("Parent device is lost").
