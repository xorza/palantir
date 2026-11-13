use crate::common::span::Span;
use crate::display::Display;
use crate::gpu::frame::frame_passes::ColorLoad;
use crate::gpu::frame::schedule::{RenderStep, RunSpan, Schedule};
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::draw_run::{DrawRun, Lane};
use crate::renderer::render_buffer::paint_tier::PaintTier;
use glam::UVec2;

const VIEWPORT: URect = URect::new(0, 0, 100, 100);

/// A run of `lane` over row `at` under `scissor`.
const fn run(lane: Lane, at: u32, scissor: Option<URect>) -> DrawRun {
    DrawRun {
        lane,
        rows: Span::new(at, 1),
        scissor,
        batch: at,
    }
}

/// A 100 × 100 frame of `runs`.
fn frame(runs: &[DrawRun]) -> RenderBuffer {
    let mut buffer = RenderBuffer::new();
    buffer.display = Display::from_physical(UVec2::splat(100), 1.0);
    buffer.runs = runs.to_vec();
    buffer
}

/// The steps a walk of `runs` of `buffer` under `damage` emits on a pass of `load`.
fn walk(
    buffer: &RenderBuffer,
    damage: Option<URect>,
    runs: RunSpan,
    load: ColorLoad,
) -> Vec<RenderStep> {
    let mut steps = Vec::new();
    Schedule {
        buffer,
        damage_scissor: damage,
        runs,
        load,
    }
    .for_each_step(&mut |step| steps.push(step));
    steps
}

/// Runs draw in the order listed, each under its scissor, the viewport where it has none, and a
/// scissor is set only where it changes: the quads and the image under `a` share one set, and the
/// text between them takes the viewport.
#[test]
fn runs_draw_as_listed_each_under_its_scissor() {
    let a = URect::new(10, 10, 50, 50);
    let quads = run(Lane::Quad, 0, Some(a));
    let text = run(Lane::Text, 0, None);
    let image = run(Lane::Tier(PaintTier::Image), 0, Some(a));
    let more = run(Lane::Quad, 1, Some(a));
    let buffer = frame(&[quads, text, image, more]);
    assert_eq!(
        walk(&buffer, None, RunSpan::ALL, ColorLoad::Load),
        [
            RenderStep::SetScissor(a),
            RenderStep::Draw(quads),
            RenderStep::SetScissor(VIEWPORT),
            RenderStep::Draw(text),
            RenderStep::SetScissor(a),
            RenderStep::Draw(image),
            RenderStep::Draw(more),
        ]
    );
}

/// A partial walk opens with the damage and the clear, then cuts each run's scissor by the
/// damage: a run inside it keeps its own, one across it takes the overlap, one with none takes
/// the damage itself, which is set already, and one past it draws nothing.
#[test]
fn the_damage_cuts_each_runs_scissor() {
    let damage = URect::new(0, 0, 40, 40);
    let inside = run(Lane::Quad, 0, Some(URect::new(5, 5, 10, 10)));
    let across = run(Lane::Text, 0, Some(URect::new(30, 30, 50, 50)));
    let whole = run(Lane::Tier(PaintTier::Mesh), 0, None);
    let past = run(Lane::Quad, 1, Some(URect::new(60, 60, 30, 30)));
    let buffer = frame(&[inside, across, whole, past]);
    assert_eq!(
        walk(&buffer, Some(damage), RunSpan::ALL, ColorLoad::Load),
        [
            RenderStep::SetScissor(damage),
            RenderStep::PreClear,
            RenderStep::SetScissor(URect::new(5, 5, 10, 10)),
            RenderStep::Draw(inside),
            RenderStep::SetScissor(URect::new(30, 30, 10, 10)),
            RenderStep::Draw(across),
            RenderStep::SetScissor(damage),
            RenderStep::Draw(whole),
        ]
    );
}

/// Only a partial walk that loads its target and starts at the first run clears: a full walk, a
/// pass that clears its target, and a walk resuming past a barrier do not.
#[test]
fn only_a_loading_partial_walk_from_the_first_run_clears() {
    let quads = run(Lane::Quad, 0, None);
    let buffer = frame(&[quads, run(Lane::Quad, 1, None)]);
    let damage = Some(VIEWPORT);
    let clears =
        |damage, runs, load| walk(&buffer, damage, runs, load).contains(&RenderStep::PreClear);
    let resumed = RunSpan {
        from: 1,
        barrier: None,
    };
    assert!(clears(damage, RunSpan::ALL, ColorLoad::Load));
    assert!(!clears(None, RunSpan::ALL, ColorLoad::Load));
    assert!(!clears(damage, RunSpan::ALL, ColorLoad::Clear));
    assert!(!clears(damage, resumed, ColorLoad::Load));
}

/// A main pass broken at a backdrop's barrier walks the runs before it, then resumes there: the
/// two walks draw what the whole walk draws, in its order. A barrier at 0 leaves the first walk
/// only the clear; one past the last run leaves the second nothing.
#[test]
fn a_walk_broken_at_a_barrier_draws_the_whole_walk_once() {
    let runs = [
        run(Lane::Quad, 0, None),
        run(Lane::Text, 0, None),
        run(Lane::Quad, 1, None),
    ];
    let buffer = frame(&runs);
    let damage = Some(VIEWPORT);
    let draws = |steps: Vec<RenderStep>| -> Vec<RenderStep> {
        steps
            .into_iter()
            .filter(|step| !matches!(step, RenderStep::SetScissor(_)))
            .collect()
    };
    let whole = draws(walk(&buffer, damage, RunSpan::ALL, ColorLoad::Load));
    assert_eq!(
        whole,
        [
            RenderStep::PreClear,
            RenderStep::Draw(runs[0]),
            RenderStep::Draw(runs[1]),
            RenderStep::Draw(runs[2]),
        ]
    );
    for barrier in 0..=3 {
        let before = RunSpan {
            from: 0,
            barrier: Some(barrier),
        };
        let after = RunSpan {
            from: barrier,
            barrier: None,
        };
        let mut both = draws(walk(&buffer, damage, before, ColorLoad::Load));
        both.extend(draws(walk(&buffer, damage, after, ColorLoad::Load)));
        let clears = both.iter().filter(|s| **s == RenderStep::PreClear).count();
        assert_eq!(clears, if barrier == 0 { 2 } else { 1 }, "at {barrier}");
        both.retain(|step| *step != RenderStep::PreClear);
        assert_eq!(both, whole[1..], "at {barrier}");
    }
}
