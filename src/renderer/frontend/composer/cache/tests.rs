use super::*;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::{color::Color, urect::URect};
use crate::renderer::gpu::buffer::{DrawGroup, TextRun};
use crate::renderer::gpu::quad::Quad;
use crate::text::TextCacheKey;
use glam::Vec2;

fn wid(n: u64) -> WidgetId {
    WidgetId(n)
}

fn hash(n: u64) -> NodeHash {
    NodeHash::from_u64(n)
}

fn avail() -> AvailableKey {
    AvailableKey { w: 800, h: 600 }
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
            quads: 0..0,
            texts: 0..0,
        };
        groups_lo as usize
    ];

    quads.push(quad(0.0));
    quads.push(quad(20.0));
    texts.push(text_run());
    groups.push(DrawGroup {
        scissor: Some(URect::new(0, 0, 100, 100)),
        quads: quads_lo..(quads_lo + 2),
        texts: texts_lo..(texts_lo + 1),
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
    assert_eq!(hit.groups[0].quads, 0..2);
    assert_eq!(hit.groups[0].texts, 0..1);
}

#[test]
fn hash_mismatch_misses() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0, 0, 0, 0);
    assert!(cache.try_lookup(wid(1), hash(2), avail(), 0).is_none());
}

#[test]
fn avail_mismatch_misses() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0, 0, 0, 0);
    assert!(
        cache
            .try_lookup(wid(1), hash(1), AvailableKey { w: 1, h: 1 }, 0)
            .is_none()
    );
}

#[test]
fn cascade_fp_mismatch_misses() {
    let mut cache = ComposeCache::default();
    write(&mut cache, wid(1), hash(1), 0xaaaa, 0, 0, 0);
    assert!(cache.try_lookup(wid(1), hash(1), avail(), 0xbbbb).is_none());
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
