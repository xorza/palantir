//! Per-frame render schedule — the ordered sequence of conceptual GPU
//! operations that paints every group in a `RenderBuffer`.
//!
//! Both production (`WgpuBackend::render_groups`) and unit tests
//! consume this same step stream via [`for_each_step`], so the order
//! asserted in tests can't drift from the order actually issued to
//! wgpu. Pure data — no GPU calls live here.

use crate::layout::types::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;

/// One conceptual step of the per-frame render schedule. Variants
/// describe *what* to do, not *how*; the consumer holds context
/// (`use_stencil`, `text_mode`, the actual `RenderPass`) to translate
/// each into wgpu calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderStep {
    /// Pre-clear quad inside the damage scissor: paints the clear
    /// color (alpha 1) over last frame's pixels so AA fringes don't
    /// compound across animation frames. Emitted only when
    /// `damage_scissor` is `Some`.
    PreClear,
    /// Narrow the render-pass scissor to this physical-px rect.
    /// Emitted both for per-group narrowing and for text-scissor
    /// expansion mid-group.
    SetScissor(URect),
    /// Set the stencil reference value (stencil-path frames only).
    /// `1` for masked-region writes, `0` for non-rounded groups and
    /// the post-draw mask clear.
    SetStencilRef(u32),
    /// Bind the mask-write pipeline + draw the mask quad at this
    /// index. Used both for the pre-draw mask stamp (at ref `1`) and
    /// the post-draw mask clear (at ref `0`) — same GPU op, different
    /// surrounding stencil refs.
    MaskQuad(u32),
    /// Bind the quad pipeline (stencil-test variant when stencil is
    /// active, plain otherwise) + draw the group's quad range.
    Quads { group: usize, range: Span },
    /// Render the group's text via the glyphon pool slot.
    Text { group: usize },
    /// Bind the mesh pipeline + issue one `draw_indexed` per
    /// `MeshDraw` in `range`. Consumer pulls per-draw spans from
    /// `RenderBuffer.meshes`.
    Meshes { group: usize, range: Span },
}

/// Walk `buffer.groups` and emit one [`RenderStep`] at a time via
/// `emit`. Pure logic — no GPU calls.
///
/// `mask_indices` parallels `buffer.groups`; index `i`'s `Some(j)`
/// says group `i`'s rounded-clip mask is mask quad `j` in the upload
/// buffer. Ignored when `use_stencil` is `false`.
///
/// Per-frame ordering invariants pinned by the emitted sequence:
///
/// 1. When `damage_scissor` is `Some`, the very first emitted steps
///    are `SetScissor(damage_scissor)` then [`PreClear`] — before
///    any group draws. AA-fringe drift would otherwise accumulate.
/// 2. Each group narrows the scissor (`SetScissor(effective)`) before
///    issuing its own draws.
/// 3. Stencil-path groups bracket their draws with mask write at
///    `stencil_ref = 1` and mask clear at `stencil_ref = 0`, so each
///    group sees a clean stencil regardless of clip ordering — except
///    when consecutive groups share the same mask, where the prior
///    group's tail clear and the new group's prologue write cancel
///    out and both are elided. The pass-final stencil is dropped
///    (`StoreOp::Discard`), so leaving a mask stamped at end of run
///    is correctness-neutral.
/// 4. Text always renders *after* its group's quads so a child quad
///    declared after a label correctly occludes that label.
/// 5. Groups whose effective scissor is empty (or doesn't intersect
///    `damage_scissor`) emit no steps at all.
///
/// [`PreClear`]: RenderStep::PreClear
pub(crate) fn for_each_step(
    buffer: &RenderBuffer,
    damage_scissor: Option<URect>,
    mask_indices: &[Option<u32>],
    use_stencil: bool,
    mut emit: impl FnMut(RenderStep),
) {
    let full_viewport = URect::new(0, 0, buffer.viewport_phys.x, buffer.viewport_phys.y);
    let text_scissor = damage_scissor.unwrap_or(full_viewport);

    if let Some(scissor) = damage_scissor {
        emit(RenderStep::SetScissor(scissor));
        emit(RenderStep::PreClear);
    }

    // `Some(mi)` means the stencil currently has mask `mi` stamped
    // (ref=1 inside the SDF, 0 outside). `None` means stencil is
    // clean and ref=0. Updated only when a group actually emits;
    // groups skipped for zero area / no damage intersect leave it
    // alone, so dedup spans across them.
    let mut active_mask: Option<u32> = None;

    for (i, g) in buffer.groups.iter().enumerate() {
        let group_scissor = g.scissor.unwrap_or(full_viewport);
        let effective = match damage_scissor {
            Some(d) => match group_scissor.intersect(d) {
                Some(r) => r,
                None => continue,
            },
            None => group_scissor,
        };
        if effective.w == 0 || effective.h == 0 {
            continue;
        }
        emit(RenderStep::SetScissor(effective));

        if use_stencil {
            let mask_idx = mask_indices[i];
            match (active_mask, mask_idx) {
                // Same mask still stamped from a prior group: skip
                // both its tail clear and this prologue write. Ref
                // is still 1 from that write.
                (Some(prev), Some(curr)) if prev == curr => {}
                // Stencil dirty from a prior mask: clear it. If this
                // group has its own mask, stamp that next.
                (Some(prev), _) => {
                    emit(RenderStep::SetStencilRef(0));
                    emit(RenderStep::MaskQuad(prev));
                    if let Some(curr) = mask_idx {
                        emit(RenderStep::SetStencilRef(1));
                        emit(RenderStep::MaskQuad(curr));
                    }
                }
                (None, Some(curr)) => {
                    emit(RenderStep::SetStencilRef(1));
                    emit(RenderStep::MaskQuad(curr));
                }
                (None, None) => {
                    emit(RenderStep::SetStencilRef(0));
                }
            }
            if g.quads.len != 0 {
                emit(RenderStep::Quads {
                    group: i,
                    range: g.quads,
                });
            }
            if g.texts.len != 0 {
                emit(RenderStep::SetScissor(text_scissor));
                emit(RenderStep::Text { group: i });
            }
            if g.meshes.len != 0 {
                // Restore the group's own scissor in case the text
                // expansion widened it; mesh draws clip against the
                // group's region same as quads.
                emit(RenderStep::SetScissor(effective));
                emit(RenderStep::Meshes {
                    group: i,
                    range: g.meshes,
                });
            }
            active_mask = mask_idx;
        } else if g.quads.len != 0 || g.texts.len != 0 || g.meshes.len != 0 {
            if g.quads.len != 0 {
                emit(RenderStep::Quads {
                    group: i,
                    range: g.quads,
                });
            }
            if g.texts.len != 0 {
                // Text uses a full-viewport scissor + per-area
                // `bounds` for clipping (set in compose). Under
                // partial repaint we narrow to the damage rect.
                emit(RenderStep::SetScissor(text_scissor));
                emit(RenderStep::Text { group: i });
            }
            if g.meshes.len != 0 {
                emit(RenderStep::SetScissor(effective));
                emit(RenderStep::Meshes {
                    group: i,
                    range: g.meshes,
                });
            }
        }
    }
}
