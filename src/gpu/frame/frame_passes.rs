//! The passes one frame records, in order, planned before any is: the backend runs the plan.

use crate::gpu::frame::main_segment::MainSegment;
use crate::gpu::frame::schedule::GroupSpan;
use crate::gpu::frame::submission::Route;

/// One step of a frame's passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PassStep {
    /// Dim what a partial frame keeps, in a pass of its own over the color target: it runs with
    /// no stencil attachment, which the main pass may have.
    Dim,
    /// One pass of the main pass, painting `groups` into the color target.
    Main {
        segment: MainSegment,
        groups: GroupSpan,
        load: ColorLoad,
    },
    /// Copy break `at`'s panes out of the color target and blur them, between two main passes.
    Break { at: usize },
    /// Copy the backbuffer onto the target.
    CopyOut,
    /// Draw the backbuffer onto a target that takes no copy.
    DrawOut,
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

/// What [`FramePasses::plan`] reads of a frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameShape {
    pub(crate) route: Route,
    /// Whether the target takes a copy, else the backbuffer is drawn onto it.
    pub(crate) takes_copy: bool,
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
        if shape.route == Route::ViaBackbuffer {
            self.steps.push(if shape.takes_copy {
                PassStep::CopyOut
            } else {
                PassStep::DrawOut
            });
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
        let load = if segment.opens() && !shape.partial {
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

    /// Whether a step writes the target itself, so the frame makes a view of it: a direct main
    /// pass, the backbuffer drawn onto it, the outline.
    pub(crate) fn writes_target(&self, route: Route) -> bool {
        self.steps.iter().any(|step| match step {
            PassStep::Main { .. } => route == Route::Direct,
            PassStep::DrawOut | PassStep::Overlay => true,
            PassStep::Dim | PassStep::Break { .. } | PassStep::CopyOut => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::frame::frame_passes::{ColorLoad, FramePasses, FrameShape, PassStep};
    use crate::gpu::frame::main_segment::MainSegment;
    use crate::gpu::frame::schedule::GroupSpan;
    use crate::gpu::frame::submission::Route;

    const fn main(
        segment: MainSegment,
        from: u32,
        barrier: Option<u32>,
        load: ColorLoad,
    ) -> PassStep {
        PassStep::Main {
            segment,
            groups: GroupSpan { from, barrier },
            load,
        }
    }

    /// Each frame shape plans its passes in record order. A whole frame is one clearing pass;
    /// a partial one loads, after its dim pass where it dims. Each barrier ends a pass and breaks,
    /// the next pass resuming at it and loading what the one before left. A frame through the
    /// backbuffer then copies it onto the target, or draws it on one that takes no copy, and the
    /// outline comes last. Only a direct main pass, a draw onto the target and the outline write
    /// the target.
    #[test]
    fn each_frame_shape_plans_its_passes_in_order() {
        use ColorLoad::{Clear, Load};
        use MainSegment::{First, Inner, Last, Whole};
        let direct_whole = FrameShape {
            route: Route::Direct,
            takes_copy: true,
            partial: false,
            dims_undamaged: false,
            outlines_damage: false,
        };
        let partial = FrameShape {
            route: Route::ViaBackbuffer,
            partial: true,
            ..direct_whole
        };
        for (label, shape, barriers, want, writes_target) in [
            (
                "a direct whole frame",
                direct_whole,
                &[][..],
                &[main(Whole, 0, None, Clear)][..],
                true,
            ),
            (
                "a partial frame",
                partial,
                &[],
                &[main(Whole, 0, None, Load), PassStep::CopyOut],
                false,
            ),
            (
                "dimmed and outlined",
                FrameShape {
                    dims_undamaged: true,
                    outlines_damage: true,
                    ..partial
                },
                &[],
                &[
                    PassStep::Dim,
                    main(Whole, 0, None, Load),
                    PassStep::CopyOut,
                    PassStep::Overlay,
                ],
                true,
            ),
            (
                "onto a target that takes no copy",
                FrameShape {
                    takes_copy: false,
                    ..partial
                },
                &[],
                &[main(Whole, 0, None, Load), PassStep::DrawOut],
                true,
            ),
            (
                "two breaks of a direct whole frame",
                direct_whole,
                &[2, 5],
                &[
                    main(First, 0, Some(2), Clear),
                    PassStep::Break { at: 0 },
                    main(Inner, 2, Some(5), Load),
                    PassStep::Break { at: 1 },
                    main(Last, 5, None, Load),
                ],
                true,
            ),
            (
                "a break at the first group",
                FrameShape {
                    route: Route::ViaBackbuffer,
                    ..direct_whole
                },
                &[0],
                &[
                    main(First, 0, Some(0), Clear),
                    PassStep::Break { at: 0 },
                    main(Last, 0, None, Load),
                    PassStep::CopyOut,
                ],
                false,
            ),
        ] {
            let mut passes = FramePasses::default();
            passes.plan(shape, barriers.iter().copied());
            assert_eq!(passes.steps(), want, "{label}");
            assert_eq!(passes.writes_target(shape.route), writes_target, "{label}");
        }
    }
}
