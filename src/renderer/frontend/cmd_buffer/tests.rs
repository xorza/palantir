use super::*;
use crate::primitives::Color;
use crate::text::TextCacheKey;

fn sample_buf() -> RenderCmdBuffer {
    let mut b = RenderCmdBuffer::new();
    b.push_clip(Rect::new(1.0, 2.0, 10.0, 20.0));
    b.draw_rect(
        Rect::new(3.0, 4.0, 5.0, 6.0),
        Corners::default(),
        Color::WHITE,
        None,
    );
    b.draw_rect(
        Rect::new(7.0, 8.0, 9.0, 10.0),
        Corners::default(),
        Color::WHITE,
        Some(Stroke {
            width: 1.0,
            color: Color::WHITE,
        }),
    );
    b.push_transform(TranslateScale::IDENTITY);
    b.draw_text(
        Rect::new(11.0, 12.0, 13.0, 14.0),
        Color::WHITE,
        TextCacheKey::INVALID,
    );
    b.pop_transform();
    b.pop_clip();
    b
}

fn rect_of(buf: &RenderCmdBuffer, i: usize) -> Rect {
    match buf.get(i) {
        RenderCmd::PushClip(r) => r,
        RenderCmd::DrawRect(p) => p.rect,
        RenderCmd::DrawRectStroked(p) => p.rect,
        RenderCmd::DrawText(p) => p.rect,
        other => panic!("no rect on {other:?}"),
    }
}

#[test]
fn extend_from_cached_shifts_rect_min() {
    let src = sample_buf();
    let mut dst = RenderCmdBuffer::new();
    let offset = Vec2::new(10.0, 20.0);
    dst.extend_from_cached(&src.kinds, &src.starts, &src.data, offset);

    assert_eq!(dst.kinds, src.kinds);
    for i in 0..src.kinds.len() {
        match src.kinds[i] {
            CmdKind::PushClip
            | CmdKind::DrawRect
            | CmdKind::DrawRectStroked
            | CmdKind::DrawText => {
                let s = rect_of(&src, i);
                let d = rect_of(&dst, i);
                assert_eq!(d.min.x, s.min.x + offset.x);
                assert_eq!(d.min.y, s.min.y + offset.y);
                assert_eq!(d.size, s.size);
            }
            CmdKind::PopClip | CmdKind::PopTransform => {}
            CmdKind::PushTransform => {
                let RenderCmd::PushTransform(d) = dst.get(i) else {
                    unreachable!()
                };
                let RenderCmd::PushTransform(s) = src.get(i) else {
                    unreachable!()
                };
                assert_eq!(d, s);
            }
        }
    }
}

#[test]
fn extend_from_cached_round_trip_is_identity() {
    let src = sample_buf();
    let mut mid = RenderCmdBuffer::new();
    mid.extend_from_cached(&src.kinds, &src.starts, &src.data, Vec2::new(7.5, -3.25));

    let mut back = RenderCmdBuffer::new();
    back.extend_from_cached(&mid.kinds, &mid.starts, &mid.data, Vec2::new(-7.5, 3.25));

    assert_eq!(back.kinds, src.kinds);
    assert_eq!(back.starts, src.starts);
    assert_eq!(back.data, src.data);
}

#[test]
fn extend_from_cached_multi_segment_concatenates() {
    let src = sample_buf();
    let mut dst = RenderCmdBuffer::new();
    dst.extend_from_cached(&src.kinds, &src.starts, &src.data, Vec2::new(1.0, 0.0));
    dst.extend_from_cached(&src.kinds, &src.starts, &src.data, Vec2::new(0.0, 1.0));

    assert_eq!(dst.kinds.len(), 2 * src.kinds.len());
    assert_eq!(dst.data.len(), 2 * src.data.len());

    let n = src.kinds.len();
    for i in 0..n {
        if matches!(
            src.kinds[i],
            CmdKind::PushClip | CmdKind::DrawRect | CmdKind::DrawRectStroked | CmdKind::DrawText
        ) {
            let s = rect_of(&src, i);
            let a = rect_of(&dst, i);
            let b = rect_of(&dst, i + n);
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
