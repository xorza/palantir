use super::*;
use crate::primitives::{Color, Corners, Rect, Stroke, TranslateScale, WidgetId};
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::test_support::{RenderCmd, cmd_at};
use crate::text::TextCacheKey;
use crate::tree::NodeHash;
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
    match cmd_at(buf, i) {
        RenderCmd::PushClip(r) => Some(r),
        RenderCmd::DrawRect(p) => Some(p.rect),
        RenderCmd::DrawRectStroked(p) => Some(p.rect),
        RenderCmd::DrawText(p) => Some(p.rect),
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
    out.extend_from_cached(hit.kinds, hit.starts, hit.data, current_origin);
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
