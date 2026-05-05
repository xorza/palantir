use super::super::cmd_buffer::RenderCmdBuffer;
use super::Composer;
use crate::layout::types::{display::Display, span::Span};
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, size::Size, stroke::Stroke,
    transform::TranslateScale, urect::URect,
};
use crate::renderer::gpu::buffer::RenderBuffer;
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

fn draw(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_rect(r, Corners::default(), Color::rgb(1.0, 1.0, 1.0), None);
}

fn text(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_text(r, Color::WHITE, TextCacheKey::INVALID);
}

fn params(scale: f32, physical: UVec2) -> Display {
    Display {
        physical,
        scale_factor: scale,
        pixel_snap: false,
    }
}

fn run(build: impl FnOnce(&mut RenderCmdBuffer), display: &Display) -> RenderBuffer {
    let mut buffer = RenderCmdBuffer::default();
    build(&mut buffer);
    let mut composer = Composer::default();
    composer.compose(&buffer, display);
    std::mem::take(&mut composer.buffer)
}

#[test]
fn compose_with_no_clip_emits_one_unscissored_group() {
    let buf = run(
        |b| {
            draw(b, rect(0.0, 0.0, 10.0, 10.0));
            draw(b, rect(20.0, 0.0, 10.0, 10.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.groups[0].scissor.is_none());
    assert_eq!(buf.groups[0].quads, Span::new(0, 2));
}

#[test]
fn compose_with_clip_groups_inner_draws_under_scissor() {
    let buf = run(
        |b| {
            draw(b, rect(0.0, 0.0, 10.0, 10.0));
            b.push_clip(rect(50.0, 50.0, 100.0, 100.0));
            draw(b, rect(60.0, 60.0, 20.0, 20.0));
            draw(b, rect(90.0, 90.0, 20.0, 20.0));
            b.pop_clip();
            draw(b, rect(0.0, 0.0, 5.0, 5.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 4);
    assert_eq!(buf.groups.len(), 3);

    assert!(buf.groups[0].scissor.is_none());
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));

    let s = buf.groups[1]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 100, 100));
    assert_eq!(buf.groups[1].quads, Span::new(1, 2));

    assert!(buf.groups[2].scissor.is_none());
    assert_eq!(buf.groups[2].quads, Span::new(3, 1));
}

#[test]
fn compose_intersects_nested_clips() {
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            b.push_clip(rect(50.0, 50.0, 100.0, 100.0));
            draw(b, rect(60.0, 60.0, 10.0, 10.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.groups.len(), 1);
    let s = buf.groups[0]
        .scissor
        .expect("nested clip group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 50, 50));
}

#[test]
fn cull_drops_drawrect_entirely_outside_active_clip() {
    // Two `DrawRect`s under the same clip: one inside, one fully
    // outside. Composer must skip emitting the outside one (the GPU
    // would scissor it, but skipping the `quads.push` saves CPU work).
    // Push/Pop pair still emits a single scissored group covering the
    // visible quad.
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(20.0, 20.0, 30.0, 30.0)); // inside
            draw(b, rect(200.0, 200.0, 30.0, 30.0)); // entirely outside
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1, "outside-clip rect must be culled");
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.groups[0].scissor.is_some());
}

#[test]
fn cull_drops_drawtext_entirely_outside_active_clip() {
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(10.0, 10.0, 50.0, 20.0)); // inside
            text(b, rect(300.0, 300.0, 50.0, 20.0)); // outside
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 1, "outside-clip text run must be culled");
}

#[test]
fn cull_keeps_drawrect_partially_inside_active_clip() {
    // Partial overlap counts — anything that could light a pixel keeps
    // its quad. Only fully-disjoint draws are dropped.
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(80.0, 80.0, 50.0, 50.0)); // straddles the clip
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1, "straddling rect must still emit");
}

#[test]
fn cull_does_not_apply_without_active_clip() {
    // No `PushClip` ⇒ no scissor active. Even far-offscreen draws
    // emit; the GPU's viewport scissor handles culling. Pin so a
    // future tightening doesn't silently start dropping unscissored
    // draws.
    let buf = run(
        |b| {
            draw(b, rect(1000.0, 1000.0, 50.0, 50.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
}

#[test]
fn cull_handles_culled_text_then_quad_split() {
    // The text-then-quad split rule lives in `GroupBuilder`. A culled
    // text run must NOT flag `last_was_text`, otherwise the next quad
    // would force a spurious group flush. Verify by drawing
    // [text-out, rect-in, rect-in] under the same clip — they should
    // share one group with both rects in it (no spurious split).
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(300.0, 300.0, 50.0, 20.0)); // culled
            draw(b, rect(10.0, 10.0, 30.0, 30.0));
            draw(b, rect(50.0, 50.0, 30.0, 30.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 0);
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(
        buf.groups.len(),
        1,
        "culled text must not flag last_was_text and split the group"
    );
}

#[test]
fn compose_skips_groups_with_no_quads() {
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 50.0, 50.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(buf.quads.is_empty());
    assert!(buf.groups.is_empty());
}

#[test]
fn compose_scales_rects_for_dpr() {
    let buf = run(
        |b| draw(b, rect(10.0, 20.0, 30.0, 40.0)),
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(20.0, 40.0));
    assert_eq!(q.rect.size, Size::new(60.0, 80.0));
}

#[test]
fn intersect_disjoint_yields_zero_size() {
    let a = URect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };
    let b = URect {
        x: 100,
        y: 100,
        w: 10,
        h: 10,
    };
    // The composer uses `URect::clamp_to` for child↔parent scissor
    // intersection — disjoint rects collapse to a zero-sized result.
    let r = b.clamp_to(a);
    assert_eq!(r.w, 0);
    assert_eq!(r.h, 0);
}

#[test]
fn compose_translates_under_push_transform() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_translation(Vec2::new(100.0, 50.0)));
            draw(b, rect(10.0, 20.0, 30.0, 40.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(110.0, 70.0));
    assert_eq!(q.rect.size, Size::new(30.0, 40.0));
}

#[test]
fn compose_scales_radius_and_stroke_under_transform() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.draw_rect(
                rect(0.0, 0.0, 50.0, 50.0),
                Corners::all(8.0),
                Color::rgb(1.0, 1.0, 1.0),
                Some(Stroke {
                    width: 1.5,
                    color: Color::rgb(0.0, 0.0, 0.0),
                }),
            );
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.size, Size::new(100.0, 100.0));
    assert_eq!(q.radius.tl, 16.0);
    assert_eq!(q.stroke.width, 3.0);
}

#[test]
fn compose_composes_nested_transforms() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.push_transform(TranslateScale::from_translation(Vec2::new(10.0, 0.0)));
            draw(b, rect(5.0, 0.0, 10.0, 10.0));
            b.pop_transform();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(30.0, 0.0));
    assert_eq!(q.rect.size, Size::new(20.0, 20.0));
}

#[test]
fn compose_transforms_clip_rects_to_screen_space() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.push_clip(rect(10.0, 10.0, 20.0, 20.0));
            draw(b, rect(15.0, 15.0, 5.0, 5.0));
            b.pop_clip();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.groups.len(), 1);
    let s = buf.groups[0]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (20, 20, 40, 40));
}

/// Pin: a `Quad → Text → Quad` paint sequence inside a single scissor
/// produces TWO groups so the second quad renders *after* the text.
/// Without this split, `submit` batches both quads together and the
/// text always paints on top — which is the bug the `text z-order`
/// showcase tab exposes.
#[test]
fn compose_splits_group_on_text_to_quad_transition() {
    let buf = run(
        |b| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(10.0, 10.0, 80.0, 20.0));
            draw(b, rect(20.0, 20.0, 60.0, 40.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(
        buf.groups.len(),
        2,
        "text→quad transition must start a new group"
    );
    // First group: quad #0 + text #0.
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));
    assert_eq!(buf.groups[0].texts, Span::new(0, 1));
    // Second group: quad #1 only — renders after group 0's text.
    assert_eq!(buf.groups[1].quads, Span::new(1, 1));
    assert_eq!(buf.groups[1].texts, Span::new(1, 0));
}

/// Pin: consecutive `Text → Text` should NOT split (both go into the
/// same group). Only `Text → Quad` triggers a flush. Otherwise a
/// header-then-body label pair produces two groups for nothing.
#[test]
fn compose_does_not_split_consecutive_texts() {
    let buf = run(
        |b| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(10.0, 10.0, 80.0, 20.0));
            text(b, rect(10.0, 35.0, 80.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.texts.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));
    assert_eq!(buf.groups[0].texts, Span::new(0, 2));
}

/// Pin: `Quad → Quad → Text` fits in one group. The text comes after
/// both quads and renders on top of both — the common case (button
/// background + button stroke + label).
#[test]
fn compose_keeps_quads_then_text_in_one_group() {
    let buf = run(
        |b| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(2.0, 2.0, 96.0, 96.0));
            text(b, rect(10.0, 10.0, 80.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, Span::new(0, 2));
    assert_eq!(buf.groups[0].texts, Span::new(0, 1));
}

mod cache_integration {
    use crate::Ui;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::color::Color;
    use crate::primitives::transform::TranslateScale;
    use crate::support::testing::{begin, ui_at};
    use crate::tree::element::Configure;
    use crate::widgets::{frame::Frame, panel::Panel, styled::Styled};
    use glam::{UVec2, Vec2};

    fn build(ui: &mut Ui) {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(8.0)
            .gap(6.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .with_id("inner")
                    .clip(true)
                    .size((Sizing::FILL, Sizing::Hug))
                    .padding(6.0)
                    .fill(Color::rgb(0.16, 0.18, 0.22))
                    .transform(TranslateScale::new(Vec2::new(2.0, 1.0), 1.0))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("a")
                            .size((Sizing::FILL, Sizing::Fixed(20.0)))
                            .fill(Color::rgb(0.4, 0.4, 0.5))
                            .show(ui);
                        Frame::new()
                            .with_id("b")
                            .size((Sizing::FILL, Sizing::Fixed(10.0)))
                            .fill(Color::rgb(0.5, 0.4, 0.4))
                            .show(ui);
                    });
            });
    }

    /// Warm-frame `RenderBuffer` (encode-cache + compose-cache both
    /// active) must be byte-identical to a cold compose with both
    /// caches cleared. Pins both the splice rebasing and the cascade
    /// fingerprint plumbing.
    #[test]
    fn compose_cache_warm_frame_matches_cold_compose() {
        let surface = UVec2::new(400, 200);
        let mut ui = ui_at(surface);
        build(&mut ui);
        ui.end_frame();

        // Frame 2: warm caches.
        begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
        let warm = ui.frontend.composer.buffer.clone();

        // Frame 3: clear both caches → cold compose under same inputs.
        crate::support::internals::clear_encode_cache(&mut ui);
        crate::support::internals::clear_compose_cache(&mut ui);
        begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
        let cold = ui.frontend.composer.buffer.clone();

        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&warm.quads),
            bytemuck::cast_slice::<_, u8>(&cold.quads),
            "quads diverge after warm cache"
        );
        assert_eq!(warm.texts.len(), cold.texts.len(), "text count diverges");
        for (i, (w, c)) in warm.texts.iter().zip(cold.texts.iter()).enumerate() {
            assert_eq!(w.origin, c.origin, "text[{i}] origin diverges");
            assert_eq!(w.bounds, c.bounds, "text[{i}] bounds diverges");
            assert_eq!(w.color, c.color, "text[{i}] color diverges");
            assert_eq!(w.key, c.key, "text[{i}] key diverges");
        }
        assert_eq!(warm.groups, cold.groups, "groups diverge after warm cache");
    }

    /// Sanity: the compose cache should have at least one snapshot
    /// after a warm frame on a non-trivial tree. Zero snapshots would
    /// invalidate the bench arms. Uses a tree large enough to clear
    /// the encoder's `TINY_SUBTREE_THRESHOLD = 4` marker emission gate.
    #[test]
    fn compose_cache_populates_on_warm_frame() {
        let surface = UVec2::new(400, 400);
        let big = |ui: &mut Ui| {
            Panel::vstack()
                .with_id("root")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..10 {
                        Panel::hstack()
                            .with_id(("row", i))
                            .size((Sizing::FILL, Sizing::Hug))
                            .show(ui, |ui| {
                                Frame::new()
                                    .with_id(("a", i))
                                    .size(Sizing::Fixed(10.0))
                                    .fill(Color::WHITE)
                                    .show(ui);
                                Frame::new()
                                    .with_id(("b", i))
                                    .size(Sizing::Fixed(10.0))
                                    .fill(Color::WHITE)
                                    .show(ui);
                                Frame::new()
                                    .with_id(("c", i))
                                    .size(Sizing::Fixed(10.0))
                                    .fill(Color::WHITE)
                                    .show(ui);
                                Frame::new()
                                    .with_id(("d", i))
                                    .size(Sizing::Fixed(10.0))
                                    .fill(Color::WHITE)
                                    .show(ui);
                            });
                    }
                });
        };

        let mut ui = ui_at(surface);
        big(&mut ui);
        ui.end_frame();
        begin(&mut ui, surface);
        big(&mut ui);
        ui.end_frame();
        assert!(
            crate::support::internals::compose_cache_snapshot_count(&ui) > 0,
            "compose cache should have populated, got {}",
            crate::support::internals::compose_cache_snapshot_count(&ui)
        );
    }

    /// Cross-cache asymmetry: `pixel_snap` is in the composer's
    /// `cascade_fp` but not in any encoder-cache key — flipping it
    /// between frames must miss the compose cache (so output reflects
    /// the new snap) yet leave the encoder cache eligible for hits.
    /// The observable contract is "warm-after-snap-flip equals cold
    /// at the new snap" — pin the cascade-fp arm of the compose key.
    #[test]
    fn compose_cache_invalidates_on_pixel_snap_flip() {
        use crate::layout::types::display::Display;
        use glam::UVec2;

        let surface = UVec2::new(400, 200);
        let snap_on = Display {
            physical: surface,
            scale_factor: 1.0,
            pixel_snap: true,
        };
        let snap_off = Display {
            pixel_snap: false,
            ..snap_on
        };

        let mut ui = Ui::new();
        ui.begin_frame(snap_on);
        build(&mut ui);
        ui.end_frame();

        // Frame 2 with snap flipped: warm caches must produce the
        // snap-off output, not a stale snap-on splice.
        ui.begin_frame(snap_off);
        build(&mut ui);
        ui.end_frame();
        let warm = ui.frontend.composer.buffer.clone();

        // Cold oracle at snap_off: clear both caches and rerun.
        crate::support::internals::clear_encode_cache(&mut ui);
        crate::support::internals::clear_compose_cache(&mut ui);
        ui.begin_frame(snap_off);
        build(&mut ui);
        ui.end_frame();
        let cold = ui.frontend.composer.buffer.clone();

        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&warm.quads),
            bytemuck::cast_slice::<_, u8>(&cold.quads),
            "snap-flip must invalidate compose cache; quads diverge",
        );
        assert_eq!(warm.groups, cold.groups);
    }

    /// Pin: clearing only the compose cache (encode cache hot) still
    /// reproduces byte-identical output. The compose-cache miss
    /// re-runs the full subtree compose; cached encode cmds drive it.
    #[test]
    fn compose_cache_clear_replays_byte_identical() {
        let surface = UVec2::new(400, 200);
        let mut ui = ui_at(surface);
        build(&mut ui);
        ui.end_frame();
        begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
        let warm = ui.frontend.composer.buffer.clone();

        crate::support::internals::clear_compose_cache(&mut ui);
        begin(&mut ui, surface);
        build(&mut ui);
        ui.end_frame();
        let cold_compose = ui.frontend.composer.buffer.clone();

        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&warm.quads),
            bytemuck::cast_slice::<_, u8>(&cold_compose.quads)
        );
        assert_eq!(warm.groups, cold_compose.groups);
    }
}
