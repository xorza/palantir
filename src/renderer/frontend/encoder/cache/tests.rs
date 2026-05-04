use super::*;
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::text::TextCacheKey;
use crate::tree::hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::Vec2;

fn buf_at(origin: Vec2) -> RenderCmdBuffer {
    let mut b = RenderCmdBuffer::default();
    b.push_clip(Rect::new(origin.x + 1.0, origin.y + 2.0, 100.0, 50.0));
    b.draw_rect(
        Rect::new(origin.x + 3.0, origin.y + 4.0, 5.0, 6.0),
        Corners::default(),
        Color::WHITE,
        None,
    );
    b.draw_rect(
        Rect::new(origin.x + 7.0, origin.y + 8.0, 9.0, 10.0),
        Corners::default(),
        Color::WHITE,
        Some(Stroke {
            width: 1.0,
            color: Color::WHITE,
        }),
    );
    b.push_transform(TranslateScale::IDENTITY);
    b.draw_text(
        Rect::new(origin.x + 11.0, origin.y + 12.0, 13.0, 14.0),
        Color::WHITE,
        TextCacheKey::INVALID,
    );
    b.pop_transform();
    b.pop_clip();
    b
}

fn rect_of(buf: &RenderCmdBuffer, i: usize) -> Option<Rect> {
    let start = buf.starts[i];
    match buf.kinds[i] {
        CmdKind::PushClip | CmdKind::DrawRect | CmdKind::DrawRectStroked | CmdKind::DrawText => {
            Some(buf.read(start))
        }
        _ => None,
    }
}

fn wid(n: u64) -> WidgetId {
    WidgetId(n)
}

fn hash(n: u64) -> NodeHash {
    NodeHash::from_u64(n)
}

fn avail() -> AvailableKey {
    AvailableKey { w: 800, h: 600 }
}

fn write_full(
    cache: &mut EncodeCache,
    w: WidgetId,
    h: NodeHash,
    src: &RenderCmdBuffer,
    origin: Vec2,
) {
    let cmd_end = src.kinds.len() as u32;
    let data_end = src.data.len() as u32;
    cache.write_subtree(
        w,
        h,
        avail(),
        src,
        Span::new(0, cmd_end),
        Span::new(0, data_end),
        origin,
    );
}

fn replay(cache: &EncodeCache, w: WidgetId, h: NodeHash, current_origin: Vec2) -> RenderCmdBuffer {
    let hit = cache.try_lookup(w, h, avail()).expect("cache miss");
    let mut out = RenderCmdBuffer::default();
    extend_from_cached(&mut out, hit.kinds, hit.starts, hit.data, current_origin);
    out
}

#[test]
fn round_trip_at_same_origin_is_byte_identical() {
    let origin = Vec2::new(50.0, 100.0);
    let src = buf_at(origin);

    let mut cache = EncodeCache::default();
    write_full(&mut cache, wid(1), hash(1), &src, origin);
    let replayed = replay(&cache, wid(1), hash(1), origin);

    assert_eq!(replayed.kinds, src.kinds);
    assert_eq!(replayed.starts, src.starts);
    assert_eq!(replayed.data, src.data);
}

#[test]
fn replay_at_shifted_origin_translates_rects() {
    let cold = buf_at(Vec2::new(50.0, 100.0));
    let mut cache = EncodeCache::default();
    write_full(&mut cache, wid(1), hash(1), &cold, Vec2::new(50.0, 100.0));

    let new_origin = Vec2::new(70.0, 130.0);
    let replayed = replay(&cache, wid(1), hash(1), new_origin);
    let expected = buf_at(new_origin);

    assert_eq!(replayed.kinds, expected.kinds);
    for i in 0..expected.kinds.len() {
        assert_eq!(rect_of(&replayed, i), rect_of(&expected, i));
    }
}

#[test]
fn hash_mismatch_misses() {
    let src = buf_at(Vec2::ZERO);
    let mut cache = EncodeCache::default();
    write_full(&mut cache, wid(1), hash(1), &src, Vec2::ZERO);
    assert!(cache.try_lookup(wid(1), hash(2), avail()).is_none());
}

#[test]
fn available_mismatch_misses() {
    let src = buf_at(Vec2::ZERO);
    let mut cache = EncodeCache::default();
    write_full(&mut cache, wid(1), hash(1), &src, Vec2::ZERO);
    let other = AvailableKey { w: 1, h: 1 };
    assert!(cache.try_lookup(wid(1), hash(1), other).is_none());
}

#[test]
fn same_len_rewrite_is_in_place() {
    let mut cache = EncodeCache::default();
    let src1 = buf_at(Vec2::new(10.0, 20.0));
    write_full(&mut cache, wid(1), hash(1), &src1, Vec2::new(10.0, 20.0));
    let snap_before = *cache.snapshots.get(&wid(1)).unwrap();
    let kinds_arena_len = cache.kinds_arena.len();
    let data_arena_len = cache.data_arena.len();

    let src2 = buf_at(Vec2::new(99.0, 88.0));
    write_full(&mut cache, wid(1), hash(1), &src2, Vec2::new(99.0, 88.0));
    let snap_after = *cache.snapshots.get(&wid(1)).unwrap();

    assert_eq!(snap_before.cmds.start, snap_after.cmds.start);
    assert_eq!(snap_before.cmds.len, snap_after.cmds.len);
    assert_eq!(snap_before.data.start, snap_after.data.start);
    assert_eq!(snap_before.data.len, snap_after.data.len);
    assert_eq!(cache.kinds_arena.len(), kinds_arena_len);
    assert_eq!(cache.data_arena.len(), data_arena_len);

    // Replay at the new origin should match a fresh cold build.
    let replayed = replay(&cache, wid(1), hash(1), Vec2::new(99.0, 88.0));
    assert_eq!(replayed.data, src2.data);
}

#[test]
fn size_change_appends_and_marks_garbage() {
    let mut cache = EncodeCache::default();

    // First snapshot: full subtree.
    let big = buf_at(Vec2::ZERO);
    write_full(&mut cache, wid(1), hash(1), &big, Vec2::ZERO);
    let big_cmds = big.kinds.len();
    let big_data = big.data.len();
    assert_eq!(cache.live_cmds, big_cmds);
    assert_eq!(cache.live_data, big_data);

    // Second snapshot: just one DrawRect. Different hash → caller would
    // see a miss before write, but write_subtree itself still rewrites
    // the same WidgetId.
    let mut small = RenderCmdBuffer::default();
    small.draw_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Corners::default(),
        Color::WHITE,
        None,
    );
    let cmd_end = small.kinds.len() as u32;
    let data_end = small.data.len() as u32;
    cache.write_subtree(
        wid(1),
        hash(2),
        avail(),
        &small,
        Span::new(0, cmd_end),
        Span::new(0, data_end),
        Vec2::ZERO,
    );

    // Live counters reflect only the new payload.
    assert_eq!(cache.live_cmds, small.kinds.len());
    assert_eq!(cache.live_data, small.data.len());
    // But the old range is still in the arena as garbage.
    assert!(cache.kinds_arena.len() > cache.live_cmds);
    assert!(cache.data_arena.len() > cache.live_data);

    // Lookup with the new hash hits and replays correctly.
    let hit = cache.try_lookup(wid(1), hash(2), avail()).unwrap();
    assert_eq!(hit.kinds.len(), 1);
}

#[test]
fn sweep_removed_evicts_and_decrements_live() {
    let mut cache = EncodeCache::default();
    let src = buf_at(Vec2::ZERO);
    write_full(&mut cache, wid(1), hash(1), &src, Vec2::ZERO);
    write_full(&mut cache, wid(2), hash(2), &src, Vec2::ZERO);
    let total_cmds = cache.live_cmds;
    let total_data = cache.live_data;

    cache.sweep_removed(&[wid(1)]);
    assert!(!cache.snapshots.contains_key(&wid(1)));
    assert!(cache.snapshots.contains_key(&wid(2)));
    assert_eq!(cache.live_cmds, total_cmds / 2);
    assert_eq!(cache.live_data, total_data / 2);
}

#[test]
fn compact_preserves_lookups() {
    let mut cache = EncodeCache::default();
    let src = buf_at(Vec2::ZERO);

    // Stuff in enough widgets to clear COMPACT_FLOOR (= 64 entries) on
    // either arena, then bust each one with a length change to leave
    // garbage behind, then write fresh entries to trip compaction.
    let n: u64 = 40;
    for i in 0..n {
        write_full(&mut cache, wid(i), hash(i), &src, Vec2::ZERO);
    }
    // Bust them all by writing a smaller payload under a new hash —
    // each leaves the original range as garbage.
    let mut small = RenderCmdBuffer::default();
    small.draw_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Corners::default(),
        Color::WHITE,
        None,
    );
    for i in 0..n {
        let cmd_end = small.kinds.len() as u32;
        let data_end = small.data.len() as u32;
        cache.write_subtree(
            wid(i),
            hash(1000 + i),
            avail(),
            &small,
            Span::new(0, cmd_end),
            Span::new(0, data_end),
            Vec2::ZERO,
        );
    }

    // After all this churn, every snapshot must still resolve and
    // replay correctly — even if compaction has run and moved their
    // arena ranges underneath.
    for i in 0..n {
        let hit = cache.try_lookup(wid(i), hash(1000 + i), avail()).unwrap();
        assert_eq!(hit.kinds.len(), 1);
        assert_eq!(hit.kinds[0], CmdKind::DrawRect);
    }
}

#[test]
fn clear_drops_everything() {
    let mut cache = EncodeCache::default();
    let src = buf_at(Vec2::ZERO);
    write_full(&mut cache, wid(1), hash(1), &src, Vec2::ZERO);
    cache.clear();
    assert_eq!(cache.live_cmds, 0);
    assert_eq!(cache.live_data, 0);
    assert!(cache.snapshots.is_empty());
    assert!(cache.kinds_arena.is_empty());
    assert!(cache.data_arena.is_empty());
    assert!(cache.try_lookup(wid(1), hash(1), avail()).is_none());
}

/// Direct unit tests for `extend_from_cached` / `bump_rect_min`. The
/// `replay_at_*` tests above exercise these via the full cache write +
/// lookup; these isolate the rect-shift behavior.
mod extend {
    use super::super::{bump_rect_min, extend_from_cached};
    use super::{buf_at, rect_of};
    use crate::primitives::transform::TranslateScale;
    use crate::renderer::frontend::cmd_buffer::{CmdKind, RenderCmdBuffer};
    use glam::Vec2;

    #[test]
    fn shifts_rect_min_on_rect_bearing_payloads() {
        let src = buf_at(Vec2::ZERO);
        let mut dst = RenderCmdBuffer::default();
        let offset = Vec2::new(10.0, 20.0);
        extend_from_cached(&mut dst, &src.kinds, &src.starts, &src.data, offset);

        assert_eq!(dst.kinds, src.kinds);
        for i in 0..src.kinds.len() {
            match src.kinds[i] {
                CmdKind::PushClip
                | CmdKind::DrawRect
                | CmdKind::DrawRectStroked
                | CmdKind::DrawText => {
                    let s = rect_of(&src, i).unwrap();
                    let d = rect_of(&dst, i).unwrap();
                    assert_eq!(d.min.x, s.min.x + offset.x);
                    assert_eq!(d.min.y, s.min.y + offset.y);
                    assert_eq!(d.size, s.size);
                }
                CmdKind::PushTransform => {
                    let d: TranslateScale = dst.read(dst.starts[i]);
                    let s: TranslateScale = src.read(src.starts[i]);
                    assert_eq!(d, s);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn round_trip_offset_is_identity() {
        let src = buf_at(Vec2::ZERO);
        let mut mid = RenderCmdBuffer::default();
        extend_from_cached(
            &mut mid,
            &src.kinds,
            &src.starts,
            &src.data,
            Vec2::new(7.5, -3.25),
        );

        let mut back = RenderCmdBuffer::default();
        extend_from_cached(
            &mut back,
            &mid.kinds,
            &mid.starts,
            &mid.data,
            Vec2::new(-7.5, 3.25),
        );

        assert_eq!(back.kinds, src.kinds);
        assert_eq!(back.starts, src.starts);
        assert_eq!(back.data, src.data);
    }

    #[test]
    fn multi_segment_concatenates_with_correct_starts() {
        let src = buf_at(Vec2::ZERO);
        let mut dst = RenderCmdBuffer::default();
        extend_from_cached(
            &mut dst,
            &src.kinds,
            &src.starts,
            &src.data,
            Vec2::new(1.0, 0.0),
        );
        extend_from_cached(
            &mut dst,
            &src.kinds,
            &src.starts,
            &src.data,
            Vec2::new(0.0, 1.0),
        );

        assert_eq!(dst.kinds.len(), 2 * src.kinds.len());
        assert_eq!(dst.data.len(), 2 * src.data.len());

        let n = src.kinds.len();
        for i in 0..n {
            if matches!(
                src.kinds[i],
                CmdKind::PushClip
                    | CmdKind::DrawRect
                    | CmdKind::DrawRectStroked
                    | CmdKind::DrawText
            ) {
                let s = rect_of(&src, i).unwrap();
                let a = rect_of(&dst, i).unwrap();
                let b = rect_of(&dst, i + n).unwrap();
                assert_eq!(a.min, Vec2::new(s.min.x + 1.0, s.min.y));
                assert_eq!(b.min, Vec2::new(s.min.x, s.min.y + 1.0));
            }
        }

        for i in 0..n {
            let s2 = dst.starts[i + n] as usize;
            assert!(s2 >= src.data.len());
            assert!(s2 <= dst.data.len());
        }
    }

    /// `bump_rect_min` must not touch payloads of kinds that don't
    /// start with a `Rect` (Pop*, Push/ExitSubtree). Round-trips a
    /// PushTransform's `TranslateScale` through a no-op shift and
    /// asserts byte-identity.
    #[test]
    fn leaves_non_rect_payloads_untouched() {
        let mut buf = RenderCmdBuffer::default();
        buf.push_transform(TranslateScale::new(Vec2::new(3.0, 5.0), 2.0));
        buf.pop_transform();
        let before = buf.data.clone();
        bump_rect_min(
            &buf.kinds,
            &buf.starts,
            &mut buf.data,
            Vec2::new(99.0, 99.0),
        );
        assert_eq!(buf.data, before);
    }
}
