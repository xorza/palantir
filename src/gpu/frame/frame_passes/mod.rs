//! The passes one frame records, in order, planned before any is: the backend runs the plan.

use crate::gpu::frame::main_segment::MainSegment;
use crate::gpu::frame::schedule::GroupSpan;

/// One step of a frame's passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PassStep {
    /// Dim what a partial frame keeps, in a pass of its own over the color target, before the
    /// main pass.
    Dim,
    /// One pass of the main pass, painting `groups` into the color target.
    Main {
        segment: MainSegment,
        groups: GroupSpan,
        load: ColorLoad,
    },
    /// Copy break `at`'s backdrops out of the color target and blur them, between two main passes.
    Break { at: usize },
    /// Copy each damage rect of the damage canvas into the backbuffer.
    CanvasOut,
    /// Put the backbuffer on the target: a copy, or a draw where the target takes none.
    BackbufferOut,
    /// Outline the damage on the target, after the backbuffer reached it, so the outline never
    /// lands in the backbuffer.
    Overlay,
}

/// What a main pass does with the color target's pixels before it paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColorLoad {
    /// Clear to the frame's clear color: the first pass of a whole frame.
    Clear,
    /// Keep them: a partial frame, or a pass after a break.
    Load,
}

/// Where a frame's main pass paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetKind {
    /// Into the target itself.
    Direct,
    /// Into the backbuffer, then onto the target.
    Backbuffer,
    /// A partial frame's damage into a damage canvas
    /// ([`DamageCanvas`](crate::gpu::surface::damage_canvas::DamageCanvas)), from a clear, then
    /// into the backbuffer, then onto the target.
    Canvas,
}

/// What [`FramePasses::plan`] reads of a frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameShape {
    pub(crate) target: TargetKind,
    /// Whether the frame repaints inside rects and keeps the pixels around them.
    pub(crate) partial: bool,
    pub(crate) dims_undamaged: bool,
    pub(crate) outlines_damage: bool,
}

/// A frame's passes, planned into retained steps.
#[derive(Debug, Default)]
pub(crate) struct FramePasses {
    steps: Vec<PassStep>,
}

impl FramePasses {
    /// Plan the passes of a frame of `shape` whose main pass breaks at `barriers`, in paint
    /// order: the dim, the main pass between its breaks, the backbuffer onto the target, the
    /// outline.
    pub(crate) fn plan(&mut self, shape: FrameShape, barriers: impl IntoIterator<Item = u32>) {
        self.steps.clear();
        let canvas = shape.target == TargetKind::Canvas;
        if canvas {
            debug_assert!(
                shape.partial,
                "a damage canvas holds a partial frame's damage"
            );
            debug_assert!(
                !shape.dims_undamaged,
                "a damage canvas keeps no pixel to dim"
            );
        }
        if shape.dims_undamaged {
            debug_assert!(shape.partial, "only a partial frame keeps pixels to dim");
            self.steps.push(PassStep::Dim);
        }
        let mut from = 0;
        let mut broken = false;
        for (at, barrier) in barriers.into_iter().enumerate() {
            let segment = if broken {
                MainSegment::Inner
            } else {
                MainSegment::First
            };
            self.push_main(shape, segment, from, Some(barrier));
            self.steps.push(PassStep::Break { at });
            from = barrier;
            broken = true;
        }
        let segment = if broken {
            MainSegment::Last
        } else {
            MainSegment::Whole
        };
        self.push_main(shape, segment, from, None);
        if canvas {
            debug_assert!(!broken, "a damage canvas holds no backdrop's capture");
            self.steps.push(PassStep::CanvasOut);
        }
        if shape.target != TargetKind::Direct {
            self.steps.push(PassStep::BackbufferOut);
        }
        if shape.outlines_damage {
            self.steps.push(PassStep::Overlay);
        }
    }

    fn push_main(
        &mut self,
        shape: FrameShape,
        segment: MainSegment,
        from: u32,
        barrier: Option<u32>,
    ) {
        let load = if segment.opens() && (!shape.partial || shape.target == TargetKind::Canvas) {
            ColorLoad::Clear
        } else {
            ColorLoad::Load
        };
        self.steps.push(PassStep::Main {
            segment,
            groups: GroupSpan { from, barrier },
            load,
        });
    }

    pub(crate) fn steps(&self) -> &[PassStep] {
        &self.steps
    }

    /// Whether a pass writes the target through a view the frame makes: a direct main or dim
    /// pass, the outline. The backbuffer makes its own where it draws onto the target.
    pub(crate) fn writes_target(&self, target: TargetKind) -> bool {
        self.steps.iter().any(|step| match step {
            PassStep::Main { .. } | PassStep::Dim => target == TargetKind::Direct,
            PassStep::Overlay => true,
            PassStep::Break { .. } | PassStep::CanvasOut | PassStep::BackbufferOut => false,
        })
    }
}

#[cfg(test)]
mod tests;
