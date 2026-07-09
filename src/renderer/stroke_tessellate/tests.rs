use crate::primitives::color::ColorU8;
use crate::renderer::stroke_tessellate::*;

fn red() -> ColorU8 {
    ColorU8::from(Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    })
}

fn green() -> ColorU8 {
    ColorU8::from(Color {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    })
}

fn blue() -> ColorU8 {
    ColorU8::from(Color {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    })
}

/// Single-color: horizontal 2-point line at width=2.
/// 8 verts (4 per cross-section), 18 indices. `seg_normal`
/// returns `(-dy, dx) = (0, +1)` for a +x segment, so
/// "outer-left" (+normal) sits at y = +1.5. AA fringe verts carry
/// the stroke's rgb at α=0 — (255,0,0,0), not transparent black —
/// so straight-alpha interpolation fades without darkening.
#[test]
fn single_horizontal_line_geometry() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 8);
    assert_eq!(i.len(), 18);
    let red_fringe = ColorU8::rgba(255, 0, 0, 0);
    assert_eq!(v[0].pos, Vec2::new(0.0, 1.5));
    assert_eq!(v[0].color, red_fringe);
    assert_eq!(v[1].pos, Vec2::new(0.0, 1.0));
    assert_eq!(v[1].color, red());
    assert_eq!(v[2].pos, Vec2::new(0.0, -1.0));
    assert_eq!(v[2].color, red());
    assert_eq!(v[3].pos, Vec2::new(0.0, -1.5));
    assert_eq!(v[3].color, red_fringe);
}

/// Hairline freeze + straight-alpha fade: only alpha scales by
/// `width_phys`; rgb is preserved. A 0.4-px white (1,1,1,1) line
/// must emit inner verts (1,1,1,0.4) — quantized:
/// round(0.4 · 255) = 102, rgb stays 255. The old premultiplied
/// convention scaled rgb too (r = 102), double-darkening once the
/// shader premultiplies.
#[test]
fn hairline_scales_only_alpha() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
        &[ColorU8::WHITE],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 0.4,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 8);
    // Geometry freezes at 1 phys px: half_geom = max(0.2, 0.5) = 0.5,
    // outer = 1.0.
    assert_eq!(v[0].pos, Vec2::new(0.0, 1.0));
    assert_eq!(v[1].pos, Vec2::new(0.0, 0.5));
    assert_eq!(v[1].color, ColorU8::rgba(255, 255, 255, 102));
    assert_eq!(v[2].color, ColorU8::rgba(255, 255, 255, 102));
    // Fringe verts: same rgb, α=0.
    assert_eq!(v[0].color, ColorU8::rgba(255, 255, 255, 0));
    assert_eq!(v[3].color, ColorU8::rgba(255, 255, 255, 0));
}

/// PerPoint: distinct colors on each cross-section, no
/// duplication.
#[test]
fn per_point_colors_distinct_per_cross_section() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
        &[red(), green(), red()],
        StrokeStyle {
            mode: ColorMode::PerPoint,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 12);
    assert_eq!(v[1].color, red());
    assert_eq!(v[5].color, green());
    assert_eq!(v[9].color, red());
}

/// PerSegment: interior cross-section duplicates; both copies
/// share position but carry the abutting segments' colors.
#[test]
fn per_segment_duplicates_interior_cross_sections() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
        &[red(), green()],
        StrokeStyle {
            mode: ColorMode::PerSegment,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 16);
    assert_eq!(v[1].color, red());
    assert_eq!(v[5].pos.x, 10.0);
    assert_eq!(v[5].color, red());
    assert_eq!(v[9].pos.x, 10.0);
    assert_eq!(v[9].color, green());
    assert_eq!(v[13].color, green());
    assert_eq!(i.len(), 36);
}

/// PerSegment strip-index correctness — pin that segment 0's
/// strip references vert blocks 0 and 1 (endpoint + trailing
/// dup), and segment 1's strip references blocks 2 and 3
/// (leading dup + endpoint).
#[test]
fn per_segment_strip_indexing_skips_join_blocks() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
        &[red(), green()],
        StrokeStyle {
            mode: ColorMode::PerSegment,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(&i[0..6], &[0, 1, 5, 0, 5, 4]);
    let last = i.len() - 6;
    assert_eq!(&i[last..], &[10, 11, 15, 10, 15, 14]);
}

/// Non-sharp join (≥ ~29°) miters as before: 4 verts per
/// cross-section, no bevel bridge.
#[test]
fn shallow_join_stays_miter() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 12);
    assert_eq!(i.len(), 36);
}

/// Sharp miter join triggers bevel chrome: 16 cross-section verts
/// + 1 bevel center + 1 concave-fill center.
#[test]
fn sharp_join_emits_bevel() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 0.5)],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 18);
    assert_eq!(i.len(), 48);
    // Bevel fringe quad references only trailing/leading blocks (4..12).
    let bridge_fringe = &i[21..27];
    for &idx in bridge_fringe {
        assert!(
            (4..12).contains(&idx),
            "bevel bridge index {idx} out of trailing/leading block range"
        );
    }
}

/// Antiparallel turn classifies as sharp via the antiparallel
/// guard in `is_sharp_join`.
#[test]
fn antiparallel_turn_is_sharp() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(-5.0, 0.0)],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 18, "antiparallel join must bevel + concave fill");
}

/// Round cap: `2*N + 3` fan verts per endpoint. width=2 ⇒ N=4, so
/// each cap contributes 11 verts and 36 indices. Also pins
/// `round_segments` directly: ceil(half_width) · 2 clamped to
/// [4, 16], computed in f32 so a huge half-width can't overflow on
/// the way to an integer (32 768 · 2 overflowed the old u16 math).
#[test]
fn round_caps_emit_fan_verts() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Round,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 30);
    assert_eq!(i.len(), 90);

    // Hand-computed: 0.5·2 = 1 and 1·2 = 2 clamp up to 4;
    // 3·2 = 6 and 8·2 = 16 pass through; 9·2 = 18 clamps to 16.
    assert_eq!(round_segments(0.5), 4);
    assert_eq!(round_segments(1.0), 4);
    assert_eq!(round_segments(3.0), 6);
    assert_eq!(round_segments(8.0), 16);
    assert_eq!(round_segments(9.0), 16);
    // Half-widths past 32 768 overflowed the old `as u16 * 2`.
    assert_eq!(round_segments(40_000.0), 16);
    assert_eq!(round_segments(f32::MAX), 16);
}

/// Round join at an interior point: dual cross-section + arc fan.
#[test]
fn round_join_emits_fan_at_interior() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        ],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Round,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 28);
    assert_eq!(i.len(), 75);
}

/// PerSegment + Round caps emits cap fans at both endpoints, with
/// the cap painted in the adjacent segment's color.
#[test]
fn per_segment_round_caps() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
        &[red(), green()],
        StrokeStyle {
            mode: ColorMode::PerSegment,
            cap: LineCap::Round,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    // 16 cross-section verts + 2 caps × 11 fan verts = 38.
    assert_eq!(v.len(), 38);
    // 2 strips × 18 + 2 caps × 36 = 108.
    assert_eq!(i.len(), 108);
    // First cap's center sits at verts[4] (after endpoint block) with red color.
    assert_eq!(v[4].pos, Vec2::ZERO);
    assert_eq!(v[4].color, red());
}

/// PerSegment + Bevel at a 90° join: dual cross-sections at the
/// interior plus bevel bridge + concave fill, with the chrome
/// painted in the average of the two segment colors.
#[test]
fn per_segment_bevel_join_emits_chrome() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        ],
        &[red(), green()],
        StrokeStyle {
            mode: ColorMode::PerSegment,
            cap: LineCap::Butt,
            join: LineJoin::Bevel,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    // 4 endpoint + 4 trailing + 4 leading + 1 bevel center + 1 concave-fill center + 4 endpoint = 18.
    assert_eq!(v.len(), 18);
    // 2 strips × 18 + bevel (3 center + 6 fringe) + concave 3 = 48.
    assert_eq!(i.len(), 48);
    // Bevel/concave-fill center is the average of red and green.
    // Layout: endpoint(0..4)+trailing(4..8)+leading(8..12)+bevel-center(12)
    //        +concave-center(13)+endpoint(14..18).
    // ColorU8 1-LSB tolerance on the linear-u8 quantization.
    let mid = v[12].color;
    let q = |x: f32| -> u8 { (x.clamp(0.0, 1.0) * 255.0).round() as u8 };
    assert!(mid.r.abs_diff(q(0.5)) <= 1);
    assert!(mid.g.abs_diff(q(0.5)) <= 1);
}

/// PerSegment + Round join: emits fan chrome painted in the
/// adjacent-segments average color.
#[test]
fn per_segment_round_join_emits_fan() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        ],
        &[red(), green()],
        StrokeStyle {
            mode: ColorMode::PerSegment,
            cap: LineCap::Butt,
            join: LineJoin::Round,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    // 4 + 4 + 4 + 11 (round fan) + 1 concave + 4 = 28.
    assert_eq!(v.len(), 28);
    // 2 strips × 18 + 1 fan × 36 + 1 concave × 3 = 75.
    assert_eq!(i.len(), 75);
}

/// Degenerate input (< 2 points) emits nothing.
#[test]
fn under_two_points_emits_nothing() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    tessellate_polyline_aa(
        &[Vec2::ZERO],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert!(v.is_empty());
    assert!(i.is_empty());
}

/// One polyline can emit more verts than a u16 index stream could
/// address. Collinear points under Miter merge to one cross-section
/// (4 verts) per point and no join chrome, so 16 384 points emit
/// 4 · 16 384 = 65 536 verts — one past the old
/// `verts <= u16::MAX = 65 535` limit that panicked here. Indices:
/// 16 383 strips × 18 = 294 894, and the largest index references
/// the last block's outer-fringe vert at 4 · 16 383 + 3 = 65 535.
#[test]
fn large_polyline_exceeds_u16_vertex_space() {
    let n = 16_384;
    let pts: Vec<Vec2> = (0..n).map(|k| Vec2::new(k as f32, 0.0)).collect();
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &pts,
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 4 * n);
    assert_eq!(i.len(), 18 * (n - 1));
    assert_eq!(i.iter().copied().max(), Some(65_535));
}

/// Indices are 0-based to this call's vert block, even when
/// the output vecs already contain other data.
#[test]
fn indices_are_zero_based_per_call() {
    let mut v = vec![MeshVertex::default(); 5];
    let mut i = vec![99u32; 3];
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(i[0..3], [99, 99, 99]);
    assert!(i[3..].iter().all(|&idx| idx < 8));
}

/// `LineCap::Square` extends both endpoints along the segment
/// direction by `inner_offset` (= width/2). Width=2 ⇒ each end
/// shifts outward by 1 phys px.
#[test]
fn square_cap_extends_endpoints_by_half_width() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Square,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    // Start cross-section was shifted to x = -1 (= 0 - inner_offset).
    assert_eq!(v[1].pos, Vec2::new(-1.0, 1.0));
    assert_eq!(v[2].pos, Vec2::new(-1.0, -1.0));
    // End cross-section was shifted to x = 11 (= 10 + inner_offset).
    assert_eq!(v[5].pos, Vec2::new(11.0, 1.0));
    assert_eq!(v[6].pos, Vec2::new(11.0, -1.0));
}

/// Explicit `LineJoin::Bevel` on a shallow turn (one that would
/// miter fine) still emits dual cross-sections + bevel chrome.
/// The `sharp_join_emits_bevel` test only exercises Miter-classified-
/// as-sharp → bevel chrome; this pins the explicit-Bevel path.
#[test]
fn bevel_join_on_shallow_turn_emits_dual_chrome() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    // 90° turn — non-sharp under Miter, but Bevel forces dual.
    tessellate_polyline_aa(
        &[
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
        ],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Bevel,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    // 4 endpoint + 4 trailing + 4 leading + 1 bevel center + 1 concave-fill center + 4 endpoint = 18.
    assert_eq!(v.len(), 18);
    // 2 strips × 18 + bevel (3 center + 6 fringe) + concave 3 = 48.
    assert_eq!(i.len(), 48);
}

/// Consecutive coincident points are filtered: 3 input points
/// where 2 coincide should produce the same mesh as the deduped
/// 2-point input.
#[test]
fn coincident_points_filtered_per_point() {
    let style = StrokeStyle {
        mode: ColorMode::PerPoint,
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        width_phys: 2.0,
    };
    let mut v_a = Vec::new();
    let mut i_a = Vec::new();
    tessellate_polyline_aa(
        // Middle point coincides with first.
        &[Vec2::ZERO, Vec2::ZERO, Vec2::new(10.0, 0.0)],
        &[red(), green(), red()],
        style,
        &mut v_a,
        &mut i_a,
    );
    let mut v_b = Vec::new();
    let mut i_b = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
        &[red(), red()],
        style,
        &mut v_b,
        &mut i_b,
    );
    assert_eq!(v_a.len(), v_b.len());
    assert_eq!(i_a, i_b);
}

/// PerSegment dedup: a coincident point's incoming segment is
/// degenerate; its color is dropped and every drawn strip paints
/// solid in the color of the segment ending at its far kept point.
#[test]
fn coincident_points_filtered_per_segment() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    // p0=p1: segment (p0,p1) red is degenerate and dropped. The one
    // drawn strip is (p1,p2) green — solid green at BOTH
    // cross-sections, not a red→green fade from the stale leading
    // color at the start point.
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::ZERO, Vec2::new(10.0, 0.0)],
        &[red(), green()],
        StrokeStyle {
            mode: ColorMode::PerSegment,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    // 4 start + 4 end = 8 verts, 18 indices (one strip).
    assert_eq!(v.len(), 8);
    assert_eq!(i.len(), 18);
    // Both cross-sections of the surviving strip are green.
    assert_eq!(v[1].color, green());
    assert_eq!(v[5].color, green());

    // Interior skip: [A, B, B, C] with segment colors [red, green,
    // blue]. Kept walk: A(0) → B(1) → C(3); segment (p1,p2) green is
    // degenerate and dropped. Collinear points + distinct colors hit
    // the smooth-miter color-boundary branch, so the layout is
    // A-block(0..4) red, trailing B-block(4..8) red, leading
    // B-block(8..12), C-block(12..16). The B→C strip must be solid
    // blue at both cross-sections: leading = cs[next_kept - 1] =
    // cs[2] = blue (the old cs[i] picked cs[1] = green and rendered
    // a green→blue fade).
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(20.0, 0.0),
        ],
        &[red(), green(), blue()],
        StrokeStyle {
            mode: ColorMode::PerSegment,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert_eq!(v.len(), 16);
    assert_eq!(i.len(), 36);
    // A→B strip solid red at both cross-sections.
    assert_eq!(v[1].color, red());
    assert_eq!(v[5].color, red());
    // B→C strip solid blue at both cross-sections.
    assert_eq!(v[9].color, blue());
    assert_eq!(v[13].color, blue());
}

/// All-coincident input emits nothing.
#[test]
fn all_coincident_input_emits_nothing() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::ZERO, Vec2::ZERO],
        &[red()],
        StrokeStyle {
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    assert!(v.is_empty());
    assert!(i.is_empty());
}

/// PerSegment with all-same colors collapses the per-point dual
/// cross-section into a single shared block at smooth-miter joins
/// — half the verts and indices vs. two-color PerSegment.
#[test]
fn per_segment_same_color_merges_cross_section() {
    let style = |mode| StrokeStyle {
        mode,
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        width_phys: 2.0,
    };
    let pts = [
        Vec2::new(0.0, 0.0),
        Vec2::new(10.0, 0.0),
        Vec2::new(20.0, 0.0),
    ];
    let mut v_same = Vec::new();
    let mut i_same = Vec::new();
    tessellate_polyline_aa(
        &pts,
        &[red(), red()],
        style(ColorMode::PerSegment),
        &mut v_same,
        &mut i_same,
    );
    let mut v_single = Vec::new();
    let mut i_single = Vec::new();
    tessellate_polyline_aa(
        &pts,
        &[red()],
        style(ColorMode::Single),
        &mut v_single,
        &mut i_single,
    );
    // Same-color PerSegment matches Single mode's geometry exactly.
    assert_eq!(v_same.len(), v_single.len());
    assert_eq!(i_same, i_single);

    // And distinctly less than two-color PerSegment (16 verts, 36 indices).
    let mut v_two = Vec::new();
    let mut i_two = Vec::new();
    tessellate_polyline_aa(
        &pts,
        &[red(), green()],
        style(ColorMode::PerSegment),
        &mut v_two,
        &mut i_two,
    );
    assert!(v_same.len() < v_two.len());
    // Index count is the same (still 2 strips), but the merge
    // saves 4 verts at the shared join.
    assert_eq!(v_two.len() - v_same.len(), 4);
}

/// PerPoint + Round caps: each end cap paints in the abutting
/// endpoint's color, not an averaged or neighbor color.
#[test]
fn per_point_round_caps_use_endpoint_color() {
    let mut v = Vec::new();
    let mut i = Vec::new();
    tessellate_polyline_aa(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
        &[red(), green(), red()],
        StrokeStyle {
            mode: ColorMode::PerPoint,
            cap: LineCap::Round,
            join: LineJoin::Miter,
            width_phys: 2.0,
        },
        &mut v,
        &mut i,
    );
    // Layout: endpoint(0..4) + cap_fan(...) + interior(4+fan..) + ... + endpoint(...) + cap_fan(...)
    // Start cap center sits right after the first endpoint cross-section.
    let first_cap_center = v[4];
    assert_eq!(first_cap_center.pos, Vec2::ZERO);
    assert_eq!(first_cap_center.color, red());
    // End cap center sits after the last endpoint block. Width=2 ⇒
    // round_segments=4 ⇒ each fan = 1 + 2·(4+1) = 11 verts. Layout:
    // 4 (start endpoint) + 11 (start cap) + 4 (interior) + 4 (end endpoint) + 11 (end cap) = 34.
    assert_eq!(v.len(), 34);
    let last_cap_center = v[4 + 11 + 4 + 4];
    assert_eq!(last_cap_center.pos, Vec2::new(20.0, 0.0));
    assert_eq!(last_cap_center.color, red());
}

/// Zero / negative / NaN widths short-circuit cleanly — no verts,
/// no indices, no NaN poisoning the output.
#[test]
fn non_positive_width_emits_nothing() {
    for width in [0.0_f32, -1.0, f32::NAN, 1e-6] {
        let mut v = Vec::new();
        let mut i = Vec::new();
        tessellate_polyline_aa(
            &[Vec2::ZERO, Vec2::new(10.0, 0.0)],
            &[red()],
            StrokeStyle {
                mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
                width_phys: width,
            },
            &mut v,
            &mut i,
        );
        assert!(v.is_empty(), "width {width} should emit no verts");
        assert!(i.is_empty(), "width {width} should emit no indices");
    }
}
