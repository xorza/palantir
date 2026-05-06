use super::*;
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::text::TextCacheKey;
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::{IVec2, Vec2};

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
    IVec2::new(800, 600)
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
    let mut out = RenderCmdBuffer::default();
    assert!(cache.try_replay(w, h, avail(), &mut out, current_origin));
    out
}

/// Replay at the same origin is byte-identical to the source; replay
/// at a shifted origin translates every rect's `min` by the delta but
/// leaves kinds untouched.
#[test]
fn replay_round_trip_cases() {
    let cases: &[(&str, Vec2, Vec2)] = &[
        (
            "same_origin",
            Vec2::new(50.0, 100.0),
            Vec2::new(50.0, 100.0),
        ),
        (
            "shifted_origin",
            Vec2::new(50.0, 100.0),
            Vec2::new(70.0, 130.0),
        ),
    ];
    for (label, write_origin, replay_origin) in cases {
        let src = buf_at(*write_origin);
        let mut cache = EncodeCache::default();
        write_full(&mut cache, wid(1), hash(1), &src, *write_origin);
        let replayed = replay(&cache, wid(1), hash(1), *replay_origin);
        let expected = buf_at(*replay_origin);

        assert_eq!(replayed.kinds, expected.kinds, "case: {label} kinds");
        for i in 0..expected.kinds.len() {
            assert_eq!(
                rect_of(&replayed, i),
                rect_of(&expected, i),
                "case: {label} rect[{i}]"
            );
        }
        if write_origin == replay_origin {
            // Same-origin replay must be byte-identical to the source —
            // no payload bits altered, no offsets shifted.
            assert_eq!(replayed.starts, src.starts, "case: {label} starts");
            assert_eq!(replayed.data, src.data, "case: {label} data");
        }
    }
}

/// `try_lookup` misses when the key fields don't all match: hash or
/// `available` differing forces a recompute even with the right
/// `WidgetId`.
#[test]
fn lookup_mismatch_misses_cases() {
    let cases: &[(&str, NodeHash, AvailableKey)] = &[
        ("hash_mismatch", hash(2), avail()),
        ("available_mismatch", hash(1), IVec2::new(1, 1)),
    ];
    for (label, h, a) in cases {
        let src = buf_at(Vec2::ZERO);
        let mut cache = EncodeCache::default();
        write_full(&mut cache, wid(1), hash(1), &src, Vec2::ZERO);
        assert!(cache.try_lookup(wid(1), *h, *a).is_none(), "case: {label}");
    }
}

#[test]
fn same_len_rewrite_is_in_place() {
    let mut cache = EncodeCache::default();
    let src1 = buf_at(Vec2::new(10.0, 20.0));
    write_full(&mut cache, wid(1), hash(1), &src1, Vec2::new(10.0, 20.0));
    let snap_before = *cache.snapshots.get(&wid(1)).unwrap();
    let kinds_arena_len = cache.kinds.items.len();
    let data_arena_len = cache.data.items.len();

    let src2 = buf_at(Vec2::new(99.0, 88.0));
    write_full(&mut cache, wid(1), hash(1), &src2, Vec2::new(99.0, 88.0));
    let snap_after = *cache.snapshots.get(&wid(1)).unwrap();

    assert_eq!(snap_before.cmds.start, snap_after.cmds.start);
    assert_eq!(snap_before.cmds.len, snap_after.cmds.len);
    assert_eq!(snap_before.data.start, snap_after.data.start);
    assert_eq!(snap_before.data.len, snap_after.data.len);
    assert_eq!(cache.kinds.items.len(), kinds_arena_len);
    assert_eq!(cache.data.items.len(), data_arena_len);

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
    assert_eq!(cache.kinds.live, big_cmds);
    assert_eq!(cache.data.live, big_data);

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
    assert_eq!(cache.kinds.live, small.kinds.len());
    assert_eq!(cache.data.live, small.data.len());
    // But the old range is still in the arena as garbage.
    assert!(cache.kinds.items.len() > cache.kinds.live);
    assert!(cache.data.items.len() > cache.data.live);

    // Lookup with the new hash hits and replays correctly.
    let hit = cache.try_lookup(wid(1), hash(2), avail()).unwrap();
    assert_eq!(hit.kinds.len(), 1);
}

#[test]
fn same_lengths_different_hash_with_kind_swap_does_not_assert() {
    // Regression: a widget's per-frame command sequence can swap kinds
    // while preserving total cmd count and data byte count — most
    // visibly in TextEdit's placeholder-Text ↔ focused-caret-Overlay
    // toggle, but reproducible at the cache layer with any same-length
    // reorder. The in-place fast path used to trigger on length
    // equality alone and assert that the cached kinds matched the new
    // kinds — wrong when the `subtree_hash` differs. Pin the fix:
    // hash-mismatch falls through to the slow append path and replay
    // returns the new kinds.
    //
    // Reproducer: the same `[DrawText, PushClip]` pair vs.
    // `[PushClip, DrawText]` — same total cmds (2) and same total
    // data words, different kind sequence.
    let mut cache = EncodeCache::default();

    let mut buf_a = RenderCmdBuffer::default();
    buf_a.draw_text(
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Color::WHITE,
        TextCacheKey::INVALID,
    );
    buf_a.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
    write_full(&mut cache, wid(1), hash(1), &buf_a, Vec2::ZERO);

    let mut buf_b = RenderCmdBuffer::default();
    buf_b.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
    buf_b.draw_text(
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Color::WHITE,
        TextCacheKey::INVALID,
    );
    // Sanity: same cmd count and same data byte count is what makes
    // the bug reachable in the first place.
    assert_eq!(buf_a.kinds.len(), buf_b.kinds.len());
    assert_eq!(buf_a.data.len(), buf_b.data.len());

    write_full(&mut cache, wid(1), hash(2), &buf_b, Vec2::ZERO);

    let hit = cache.try_lookup(wid(1), hash(2), avail()).unwrap();
    assert_eq!(hit.kinds, &[CmdKind::PushClip, CmdKind::DrawText]);
}

#[test]
fn sweep_removed_evicts_and_decrements_live() {
    let mut cache = EncodeCache::default();
    let src = buf_at(Vec2::ZERO);
    write_full(&mut cache, wid(1), hash(1), &src, Vec2::ZERO);
    write_full(&mut cache, wid(2), hash(2), &src, Vec2::ZERO);
    let total_cmds = cache.kinds.live;
    let total_data = cache.data.live;

    cache.sweep_removed(&[wid(1)]);
    assert!(!cache.snapshots.contains_key(&wid(1)));
    assert!(cache.snapshots.contains_key(&wid(2)));
    assert_eq!(cache.kinds.live, total_cmds / 2);
    assert_eq!(cache.data.live, total_data / 2);
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

    // Compaction must have actually run during the churn — without it
    // the data arena would carry every busted big-payload range as
    // garbage (~12 words each × 40 writes, ~10× live). The trigger
    // bounds items at ≤ 2× live by design, so asserting that bound
    // confirms compaction kicked in at least once.
    assert!(cache.data.items.len() <= cache.data.live * 2);

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
    assert_eq!(cache.kinds.live, 0);
    assert_eq!(cache.data.live, 0);
    assert!(cache.snapshots.is_empty());
    assert!(cache.kinds.items.is_empty());
    assert!(cache.data.items.is_empty());
    assert!(cache.try_lookup(wid(1), hash(1), avail()).is_none());
}

/// Two consecutive `try_replay` calls for the same widget must
/// concatenate cleanly: each appended slice's `start`s rebased onto the
/// growing data arena, and each segment's `rect.min` shifted by its own
/// offset. Pins the `dest_data_base` math under repeated replay.
#[test]
fn try_replay_twice_concatenates_with_correct_starts() {
    let src = buf_at(Vec2::ZERO);
    let mut cache = EncodeCache::default();
    write_full(&mut cache, wid(1), hash(1), &src, Vec2::ZERO);

    let mut dst = RenderCmdBuffer::default();
    assert!(cache.try_replay(wid(1), hash(1), avail(), &mut dst, Vec2::new(1.0, 0.0)));
    assert!(cache.try_replay(wid(1), hash(1), avail(), &mut dst, Vec2::new(0.0, 1.0)));

    assert_eq!(dst.kinds.len(), 2 * src.kinds.len());
    assert_eq!(dst.data.len(), 2 * src.data.len());

    let n = src.kinds.len();
    for i in 0..n {
        if let (Some(s), Some(a), Some(b)) =
            (rect_of(&src, i), rect_of(&dst, i), rect_of(&dst, i + n))
        {
            assert_eq!(a.min, Vec2::new(s.min.x + 1.0, s.min.y));
            assert_eq!(b.min, Vec2::new(s.min.x, s.min.y + 1.0));
        }
        let s2 = dst.starts[i + n] as usize;
        assert!(s2 >= src.data.len() && s2 <= dst.data.len());
    }
}

/// `bump_rect_min` must not touch payloads of kinds that don't start
/// with a `Rect` (Pop*, Push/ExitSubtree). Pin via a buffer of
/// PushTransform + Pop and a no-op-shift round trip.
#[test]
fn bump_rect_min_leaves_non_rect_payloads_untouched() {
    let mut buf = RenderCmdBuffer::default();
    buf.push_transform(TranslateScale::new(Vec2::new(3.0, 5.0), 2.0));
    buf.pop_transform();
    let before = buf.data.clone();
    super::bump_rect_min(
        &buf.kinds,
        &buf.starts,
        &mut buf.data,
        Vec2::new(99.0, 99.0),
    );
    assert_eq!(buf.data, before);
}

/// Cached subtrees may be replayed at a different absolute buffer
/// position than recording time. The composer reads
/// `EnterSubtreePayload.exit_idx` to fast-forward past the matching
/// `ExitSubtree`; if the snapshot stored absolute kind-indices,
/// the fast-forward would land on the wrong cmd in the replayed
/// buffer, leaving unmatched `Push/PopClip` pairs and panicking.
///
/// Pin the round-trip: build a buffer with leading cmds + an
/// EnterSubtree/body/ExitSubtree triple, snapshot it, replay it
/// into a fresh buffer with DIFFERENT leading cmds, and verify the
/// replayed `exit_idx` points to the actual `ExitSubtree` position
/// in the new buffer.
#[test]
fn replay_rebases_enter_subtree_exit_idx_to_destination_position() {
    use crate::renderer::frontend::cmd_buffer::EnterSubtreePayload;

    fn read_exit_idx(buf: &RenderCmdBuffer, kind_idx: usize) -> u32 {
        assert!(matches!(buf.kinds[kind_idx], CmdKind::EnterSubtree));
        let payload: EnterSubtreePayload = buf.read(buf.starts[kind_idx]);
        payload.exit_idx
    }

    // Source: 2 leading cmds + Enter at idx 2, body cmd, Exit at idx 4.
    let mut src = RenderCmdBuffer::default();
    src.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
    src.pop_clip();
    let cmd_lo_src = src.kinds.len() as u32;
    let patch = src.push_enter_subtree(wid(7), hash(7), avail());
    src.draw_rect(
        Rect::new(1.0, 2.0, 3.0, 4.0),
        Corners::default(),
        Color::WHITE,
        None,
    );
    src.push_exit_subtree(patch);
    let exit_idx_src = (src.kinds.len() - 1) as u32;
    assert_eq!(
        read_exit_idx(&src, cmd_lo_src as usize),
        exit_idx_src,
        "sanity: source exit_idx points to ExitSubtree position pre-cache"
    );

    // Snapshot only the subtree cmds (idx 2..5).
    let mut cache = EncodeCache::default();
    cache.write_subtree(
        wid(7),
        hash(7),
        avail(),
        &src,
        Span::new(cmd_lo_src, src.kinds.len() as u32 - cmd_lo_src),
        Span::new(0, src.data.len() as u32), // whole data — start helper takes ranges
        Vec2::ZERO,
    );

    // Replay into a buffer with a DIFFERENT leading prefix (5 dummy
    // cmds). The cached subtree should land at idx 5; its EnterSubtree
    // is at idx 5 and its ExitSubtree is at idx 5 + 2 = 7.
    let mut dst = RenderCmdBuffer::default();
    for _ in 0..5 {
        dst.push_clip(Rect::ZERO);
        dst.pop_clip();
    }
    // Even number of clip-cmds so we don't unbalance dst — 2 cmds × 5 = 10.
    let kinds_base = dst.kinds.len() as u32;
    assert!(cache.try_replay(wid(7), hash(7), avail(), &mut dst, Vec2::ZERO));

    let new_enter_idx = kinds_base as usize;
    let expected_exit_idx = kinds_base + 2; // body cmd + Exit; relative offset 2
    assert_eq!(
        read_exit_idx(&dst, new_enter_idx),
        expected_exit_idx,
        "replay rebased exit_idx to point at the matching ExitSubtree in dst"
    );
}
