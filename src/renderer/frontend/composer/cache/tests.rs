use super::*;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::{color::Color, urect::URect};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{DrawGroup, TextRun};
use crate::text::TextCacheKey;
use glam::{IVec2, Vec2};

fn wid(n: u64) -> WidgetId {
    WidgetId(n)
}

fn hash(n: u64) -> NodeHash {
    NodeHash::from_u64(n)
}

fn avail() -> AvailableKey {
    IVec2::new(800, 600)
}

fn quad(x: f32) -> Quad {
    Quad::new(
        Rect::new(x, 0.0, 10.0, 10.0),
        Color::WHITE,
        Corners::default(),
        None,
    )
}

fn text_run() -> TextRun {
    TextRun {
        origin: Vec2::ZERO,
        bounds: URect::new(0, 0, 10, 10),
        color: Color::WHITE,
        key: TextCacheKey::INVALID,
    }
}

/// Build a tiny in-buffer subtree starting at the given live offsets,
/// then call `write_subtree` to copy it into the cache. Returns the
/// quads/texts/groups vectors so tests can compare splice output.
fn write(
    cache: &mut ComposeCache,
    w: WidgetId,
    h: NodeHash,
    fp: u64,
    quads_lo: u32,
    texts_lo: u32,
    groups_lo: u32,
) -> (Vec<Quad>, Vec<TextRun>, Vec<DrawGroup>) {
    // Pre-pad with `*_lo` dummy entries so the *_lo offsets address
    // the right slot when write_subtree subtracts them.
    let mut quads = vec![quad(-1.0); quads_lo as usize];
    let mut texts = vec![text_run(); texts_lo as usize];
    let mut groups = vec![
        DrawGroup {
            scissor: None,
            rounded_clip: None,
            quads: Span::new(0, 0),
            texts: Span::new(0, 0),
        };
        groups_lo as usize
    ];

    quads.push(quad(0.0));
    quads.push(quad(20.0));
    texts.push(text_run());
    groups.push(DrawGroup {
        scissor: Some(URect::new(0, 0, 100, 100)),
        rounded_clip: None,
        quads: Span::new(quads_lo, 2),
        texts: Span::new(texts_lo, 1),
    });

    cache.write_subtree(
        w,
        h,
        avail(),
        fp,
        &quads[quads_lo as usize..],
        &texts[texts_lo as usize..],
        &groups[groups_lo as usize..],
        quads_lo,
        texts_lo,
    );
    (quads, texts, groups)
}

#[test]
fn round_trip_lookup_returns_subtree_relative_groups() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0xdead, 5, 3, 2);

    let hit = cache.try_lookup(wid(1), hash(1), avail(), 0xdead).unwrap();
    assert_eq!(hit.quads.len(), 2);
    assert_eq!(hit.texts.len(), 1);
    assert_eq!(hit.groups.len(), 1);
    // Group ranges stored as 0-based (subtree-relative).
    assert_eq!(hit.groups[0].quads, Span::new(0, 2));
    assert_eq!(hit.groups[0].texts, Span::new(0, 1));
}

/// `try_lookup` misses when any of the key fields disagrees: hash,
/// `available`, or cascade fingerprint.
#[test]
fn lookup_mismatch_misses_cases() {
    type LookupKey = (NodeHash, AvailableKey, u64);
    let cases: &[(&str, u64, LookupKey)] = &[
        ("hash_mismatch", 0, (hash(2), avail(), 0)),
        ("avail_mismatch", 0, (hash(1), IVec2::new(1, 1), 0)),
        ("cascade_fp_mismatch", 0xaaaa, (hash(1), avail(), 0xbbbb)),
    ];
    for (label, written_fp, (h, a, fp)) in cases {
        let mut cache = ComposeCache::default();
        write(&mut cache, wid(1), hash(1), *written_fp, 0, 0, 0);
        assert!(
            cache.try_lookup(wid(1), *h, *a, *fp).is_none(),
            "case: {label}"
        );
    }
}

#[test]
fn same_len_rewrite_is_in_place() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0, 0, 0, 0);
    let snap_before = *cache.snapshots.get(&wid(1)).unwrap();

    write(&mut cache, wid(1), hash(2), 0xff, 0, 0, 0);
    let snap_after = *cache.snapshots.get(&wid(1)).unwrap();

    assert_eq!(snap_before.quads.start, snap_after.quads.start);
    assert_eq!(snap_before.texts.start, snap_after.texts.start);
    assert_eq!(snap_before.groups.start, snap_after.groups.start);
    assert_eq!(snap_after.subtree_hash, hash(2));
    assert_eq!(snap_after.cascade_fp, 0xff);
}

#[test]
fn size_change_appends_and_marks_garbage() {
    let mut cache = ComposeCache::default();

    // First snapshot: 2 quads, 1 text, 1 group (the helper's shape).
    write(&mut cache, wid(1), hash(1), 0xa, 0, 0, 0);
    let big_q = cache.quads.live;
    let big_t = cache.texts.live;
    let big_g = cache.groups.live;
    assert_eq!(big_q, 2);
    assert_eq!(big_t, 1);
    assert_eq!(big_g, 1);

    // Second snapshot under same wid but different lengths: 1 quad,
    // 0 texts, 1 group. Triggers the append + garbage path.
    let small_quads = vec![quad(0.0)];
    let small_groups = vec![DrawGroup {
        scissor: None,
        rounded_clip: None,
        quads: Span::new(0, 1),
        texts: Span::new(0, 0),
    }];
    cache.write_subtree(
        wid(1),
        hash(2),
        avail(),
        0xb,
        &small_quads,
        &[],
        &small_groups,
        0,
        0,
    );

    // Live counters reflect only the new payload.
    assert_eq!(cache.quads.live, 1);
    assert_eq!(cache.texts.live, 0);
    assert_eq!(cache.groups.live, 1);
    // Old ranges still in arenas as garbage.
    assert!(cache.quads.items.len() > cache.quads.live);
    assert!(cache.groups.items.len() > cache.groups.live);

    // Lookup with the new key hits and replays the small shape.
    let hit = cache.try_lookup(wid(1), hash(2), avail(), 0xb).unwrap();
    assert_eq!(hit.quads.len(), 1);
    assert_eq!(hit.texts.len(), 0);
    assert_eq!(hit.groups.len(), 1);
}

#[test]
fn sweep_removed_evicts_and_decrements_live() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0, 0, 0, 0);
    write(&mut cache, wid(2), hash(2), 0, 0, 0, 0);
    let total_q = cache.quads.live;
    let total_t = cache.texts.live;
    let total_g = cache.groups.live;

    cache.sweep_removed(&[wid(1)]);
    assert!(!cache.snapshots.contains_key(&wid(1)));
    assert!(cache.snapshots.contains_key(&wid(2)));
    assert_eq!(cache.quads.live, total_q / 2);
    assert_eq!(cache.texts.live, total_t / 2);
    assert_eq!(cache.groups.live, total_g / 2);
}

#[test]
fn clear_drops_everything() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0, 0, 0, 0);
    cache.clear();
    assert_eq!(cache.quads.live, 0);
    assert_eq!(cache.texts.live, 0);
    assert_eq!(cache.groups.live, 0);
    assert!(cache.snapshots.is_empty());
    assert!(cache.try_lookup(wid(1), hash(1), avail(), 0).is_none());
}

/// Mirrors `EncodeCache::compact_preserves_lookups`. Stuff in enough
/// snapshots to clear `COMPACT_FLOOR`, bust each one with a
/// shorter-length write to leave garbage, then verify the per-arena
/// invariant holds and every snapshot still resolves to the right
/// payload after compaction has run.
#[test]
fn compact_preserves_lookups() {
    use crate::common::cache_arena::COMPACT_FLOOR;

    let mut cache = ComposeCache::default();
    let n = (COMPACT_FLOOR as u64) + 8;
    for i in 0..n {
        write(&mut cache, wid(i), hash(i), 0xa, 0, 0, 0);
    }

    // Bust each one under a new (hash, fp) with a shorter payload —
    // 1 quad / 0 text / 1 group. Triggers the append-with-garbage
    // path; compaction kicks in once an arena crosses `live * RATIO`.
    let small_quads = vec![quad(0.0)];
    let small_groups = vec![DrawGroup {
        scissor: None,
        rounded_clip: None,
        quads: Span::new(0, 1),
        texts: Span::new(0, 0),
    }];
    for i in 0..n {
        cache.write_subtree(
            wid(i),
            hash(1000 + i),
            avail(),
            0xb,
            &small_quads,
            &[],
            &small_groups,
            0,
            0,
        );
    }

    // Compaction must have run on at least one arena: without it the
    // quads arena would still hold every busted 2-quad range as
    // garbage (~3× live). The trigger bounds items at ≤ 2× live.
    assert!(cache.quads.items.len() <= cache.quads.live * 2);
    assert!(cache.groups.items.len() <= cache.groups.live * 2);

    // Every snapshot still resolves and matches its rewritten shape.
    for i in 0..n {
        let hit = cache
            .try_lookup(wid(i), hash(1000 + i), avail(), 0xb)
            .unwrap();
        assert_eq!(hit.quads.len(), 1);
        assert_eq!(hit.texts.len(), 0);
        assert_eq!(hit.groups.len(), 1);
        // Subtree-relative group ranges stay valid post-compaction
        // because compaction moves the whole range, not the offsets
        // within it.
        assert_eq!(hit.groups[0].quads, Span::new(0, 1));
    }
}

/// Pin the `try_splice` rebase math: splicing the same snapshot twice
/// into a buffer that already holds an unrelated quad and text run
/// must rebase each appended group's `quads.start` / `texts.start` to
/// the post-extend offsets — independently per arena, since `out.quads`
/// and `out.texts` grow at different rates.
#[test]
fn try_splice_rebases_groups_against_growing_buffer() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0xfe, 0, 0, 0);

    let mut out = RenderBuffer::default();
    // Pre-populate `out` with a non-symmetric prefix: 3 quads, 1 text.
    // The text base and quad base diverge so a buggy rebase that mixes
    // them up shows.
    out.quads
        .extend_from_slice(&[quad(100.0), quad(110.0), quad(120.0)]);
    out.texts.push(text_run());

    // First splice — group ranges should rebase by (3, 1).
    assert!(cache.try_splice(wid(1), hash(1), avail(), 0xfe, &mut out));
    assert_eq!(out.quads.len(), 5, "splice appended 2 quads");
    assert_eq!(out.texts.len(), 2, "splice appended 1 text");
    assert_eq!(out.groups.len(), 1);
    assert_eq!(out.groups[0].quads, Span::new(3, 2));
    assert_eq!(out.groups[0].texts, Span::new(1, 1));

    // Second splice on top — must rebase against the new tails (5, 2).
    assert!(cache.try_splice(wid(1), hash(1), avail(), 0xfe, &mut out));
    assert_eq!(out.quads.len(), 7);
    assert_eq!(out.texts.len(), 3);
    assert_eq!(out.groups.len(), 2);
    assert_eq!(out.groups[1].quads, Span::new(5, 2));
    assert_eq!(out.groups[1].texts, Span::new(2, 1));
}

/// `try_splice` against an empty `RenderBuffer` must produce a group
/// whose ranges match the snapshot's subtree-relative ranges verbatim
/// — base offsets are zero, so no rebase shift is observable.
#[test]
fn try_splice_into_empty_buffer_preserves_relative_groups() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0, 0, 0, 0);
    let mut out = RenderBuffer::default();
    assert!(cache.try_splice(wid(1), hash(1), avail(), 0, &mut out));
    assert_eq!(out.groups[0].quads, Span::new(0, 2));
    assert_eq!(out.groups[0].texts, Span::new(0, 1));
}
