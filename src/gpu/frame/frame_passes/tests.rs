use crate::gpu::frame::frame_passes::{ColorLoad, FramePasses, FrameShape, PassStep, TargetKind};
use crate::gpu::frame::main_segment::MainSegment;
use crate::gpu::frame::schedule::GroupSpan;

const fn main(segment: MainSegment, from: u32, barrier: Option<u32>, load: ColorLoad) -> PassStep {
    PassStep::Main {
        segment,
        groups: GroupSpan { from, barrier },
        load,
    }
}

/// Each frame shape plans its passes in record order. A whole frame is one clearing pass;
/// a partial one loads, after its dim pass where it dims, but on a damage canvas, which it
/// clears and then copies into the backbuffer. Each barrier ends a pass and breaks,
/// the next pass resuming at it and loading what the one before left. A frame through the
/// backbuffer then puts it on the target, and the outline comes last. Only a direct main pass
/// and the outline write the target through the frame's view of it.
#[test]
fn each_frame_shape_plans_its_passes_in_order() {
    use ColorLoad::{Clear, Load};
    use MainSegment::{First, Inner, Last, Whole};
    let direct_whole = FrameShape {
        target: TargetKind::Direct,
        partial: false,
        dims_undamaged: false,
        outlines_damage: false,
    };
    let partial = FrameShape {
        target: TargetKind::Backbuffer,
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
            &[main(Whole, 0, None, Load), PassStep::BackbufferOut],
            false,
        ),
        (
            "a partial frame on a damage canvas, outlined",
            FrameShape {
                target: TargetKind::Canvas,
                outlines_damage: true,
                ..partial
            },
            &[],
            &[
                main(Whole, 0, None, Clear),
                PassStep::CanvasOut,
                PassStep::BackbufferOut,
                PassStep::Overlay,
            ],
            true,
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
                PassStep::BackbufferOut,
                PassStep::Overlay,
            ],
            true,
        ),
        (
            "a direct partial frame, dimmed, on a target that keeps its pixels",
            FrameShape {
                target: TargetKind::Direct,
                dims_undamaged: true,
                ..partial
            },
            &[],
            &[PassStep::Dim, main(Whole, 0, None, Load)],
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
                target: TargetKind::Backbuffer,
                ..direct_whole
            },
            &[0],
            &[
                main(First, 0, Some(0), Clear),
                PassStep::Break { at: 0 },
                main(Last, 0, None, Load),
                PassStep::BackbufferOut,
            ],
            false,
        ),
    ] {
        let mut passes = FramePasses::default();
        passes.plan(shape, barriers.iter().copied());
        assert_eq!(passes.steps(), want, "{label}");
        assert_eq!(passes.writes_target(shape.target), writes_target, "{label}");
    }
}
