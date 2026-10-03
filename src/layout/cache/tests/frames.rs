//! Cache × full-frame integration: records widget trees across two
//! frames at the same surface and asserts the warm-cache frame
//! reproduces the cold-frame layout (and encoded commands). Catches
//! per-frame engine state we forgot to snapshot/restore on a cache
//! hit.

use crate::primitives::widget_id::WidgetId;
use crate::text::font_scope::internals::INTER;
use crate::text::wrap::TextWrap;

use crate::TextStyle;
use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::internals::paint_capture::internals::assert_same_capture;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::background::Background;
use crate::primitives::shadow::Shadow;
use crate::primitives::{
    color::RgbaF32, corners::Corners, stroke::Stroke, translate_scale::TranslateScale,
};
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::widgets::configure::Configure;
use crate::widgets::{
    block::Block, button::Button, grid::Grid, panel::Panel, scroll::Scroll, text::Text,
};
use glam::UVec2;

/// Run `record` twice at `size` (cold then warm-from-cache) and assert
/// every captured node's arranged rect matches across the two frames.
/// `record` pushes the nodes whose rects matter into `capture`.
fn assert_warm_rects_match_cold(
    h: &mut UiHarness,
    size: UVec2,
    msg: &str,
    mut record: impl FnMut(&mut Ui, &mut Vec<NodeId>),
) {
    h.resize(size);
    let cold_nodes = h.frame_value(|ui| {
        let mut nodes = Vec::new();
        record(ui, &mut nodes);
        nodes
    });
    let cold: Vec<_> = cold_nodes
        .iter()
        .map(|&n| h.ui.arranged_rect(Layer::Main, n))
        .collect();

    let warm_nodes = h.frame_value(|ui| {
        let mut nodes = Vec::new();
        record(ui, &mut nodes);
        nodes
    });
    let warm: Vec<_> = warm_nodes
        .iter()
        .map(|&n| h.ui.arranged_rect(Layer::Main, n))
        .collect();

    // Guard against the test going inert: if hash stability ever
    // regresses and the warm frame misses everywhere, cold == warm
    // would pass vacuously while pinning nothing.
    assert!(
        !h.engines.layout.scratch.counters.cache_hits().is_empty(),
        "warm frame produced no measure-cache hits — {msg} pins nothing",
    );
    assert_eq!(cold, warm, "{msg}");
}

/// Cross-frame measure-cache regression. When the cache hits at a
/// Grid (or any ancestor), the grid driver's per-frame `GridTrackStore`
/// scratch must be re-populated from the snapshot — otherwise arrange
/// computes zero column widths, collapsing every cell to x=0.
///
/// Topologies pinned: a single grid, nested grids (outer + inner), and
/// two sibling grids inside a vstack (cache hit must restore tracks for
/// both, in pre-order).
#[test]
fn cache_hit_preserves_grid_cell_rects() {
    type Build = fn(&mut Ui, &mut Vec<NodeId>);
    let cases: &[(&str, Build)] = &[
        ("single_grid", |ui, capture| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("g"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .cols([Track::HUG, Track::FILL])
                        .rows([Track::HUG])
                        .line_gap(6.0)
                        .gap(16.0)
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("Title:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            capture.push(
                                Text::new("value column")
                                    .auto_id()
                                    .font_size(14.0)
                                    .text_wrap(TextWrap::WrapWithOverflow)
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node(),
                            );
                        });
                });
        }),
        ("nested_grids", |ui, capture| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("outer"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .cols([Track::HUG, Track::FILL])
                        .rows([Track::HUG])
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("outer-L")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            Panel::vstack()
                                .id(WidgetId::from_hash("inner-host"))
                                .grid_cell((0, 1))
                                .show(ui, |ui| {
                                    Grid::new()
                                        .id(WidgetId::from_hash("inner"))
                                        .size((Sizing::FILL, Sizing::HUG))
                                        .cols([Track::HUG, Track::HUG, Track::FILL])
                                        .rows([Track::HUG])
                                        .show(ui, |ui| {
                                            for (col, label) in [(0, "a"), (1, "bb"), (2, "end")] {
                                                capture.push(
                                                    Text::new(label)
                                                        .id(WidgetId::from_hash((
                                                            "inner-cell",
                                                            col,
                                                        )))
                                                        .style(
                                                            &TextStyle::default()
                                                                .with_font_size(14.0),
                                                        )
                                                        .grid_cell((0, col))
                                                        .show(ui)
                                                        .node(),
                                                );
                                            }
                                        });
                                });
                        });
                });
        }),
        ("sibling_grids", |ui, capture| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("g1"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .cols([Track::HUG, Track::FILL])
                        .rows([Track::HUG])
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("L1:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            capture.push(
                                Text::new("v1")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node(),
                            );
                        });
                    Grid::new()
                        .id(WidgetId::from_hash("g2"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .cols([Track::HUG, Track::HUG, Track::FILL])
                        .rows([Track::HUG])
                        .show(ui, |ui| {
                            capture.push(
                                Text::new("Description:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            capture.push(
                                Text::new("end")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 2))
                                    .show(ui)
                                    .node(),
                            );
                        });
                });
        }),
    ];
    for (label, record) in cases {
        let mut h = UiHarness::with_text(UVec2::new(800, 600));
        assert_warm_rects_match_cold(
            &mut h,
            UVec2::new(800, 600),
            &format!("case: {label}"),
            *record,
        );
        // The hit must land at the synthetic viewport root (an
        // ancestor of every grid) — only then is the grid's measure
        // skipped entirely and the hug-restore path actually
        // exercised. A descendant-level hit would re-run grid measure
        // and pin nothing.
        assert!(
            h.engines
                .layout
                .scratch
                .counters
                .cache_hits()
                .contains(&WidgetId::VIEWPORT),
            "case {label}: warm cache hit didn't land at the viewport root — grid hug \
             restore not exercised. hits={:?}",
            h.engines.layout.scratch.counters.cache_hits(),
        );
    }
}

/// Per-driver cache-hit defense. Today only Grid retains per-subtree
/// measure→arrange state (see `LayoutScratch` docs on the cache-hit
/// contract); the other drivers drain their scratch on measure exit
/// so a cache hit at an ancestor is structurally invisible to them.
/// This test pins that property — any future driver that accidentally
/// adds category-(2) state without wiring it through
/// [`LayoutScratch::restore_after_cache_hit`] will desync these
/// fixtures' warm rects from cold.
///
/// Each case builds a minimal subtree under the named driver, runs
/// the same `record` cold then warm at the same surface, and asserts
/// the captured leaf rects match. The cache is shared across the two
/// frames inside one `Ui`, so the second frame's outer Panel cache
/// hit forces the driver's measure to be skipped — if its arrange
/// reads stale or zero state, the warm rect will diverge.
#[test]
fn cache_hit_preserves_per_driver_rects() {
    type Build = fn(&mut Ui, &mut Vec<NodeId>);
    let cases: &[(&str, Build)] = &[
        ("hstack", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("row"))
                    .gap(6.0)
                    .show(ui, |ui| {
                        for (i, label) in ["alpha", "beta", "gamma"].iter().enumerate() {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("cell", i)))
                                    .font_size(14.0)
                                    .show(ui)
                                    .node(),
                            );
                        }
                    });
            });
        }),
        ("vstack_fill_freeze", |ui, capture| {
            // Three Fill children with min-content floors that force
            // the freeze loop. Stack measure pushes onto
            // `stack.fill`; a cache hit at the outer panel skips
            // the freeze entirely, so arrange must still read correct
            // per-child slots from `desired` alone.
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("freeze"))
                    .size((Sizing::fixed(200.0), Sizing::HUG))
                    .show(ui, |ui| {
                        for (i, label) in [
                            "needs-some-room-here",
                            "wider-than-share-A",
                            "wider-than-share-B",
                        ]
                        .iter()
                        .enumerate()
                        {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("fill", i)))
                                    .size((Sizing::fill(1.0), Sizing::HUG))
                                    .font_size(14.0)
                                    .show(ui)
                                    .node(),
                            );
                        }
                    });
            });
        }),
        ("wrap_hstack", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::wrap_hstack()
                    .id(WidgetId::from_hash("wrap"))
                    .size((Sizing::fixed(120.0), Sizing::HUG))
                    .gap(4.0)
                    .line_gap(4.0)
                    .show(ui, |ui| {
                        for (i, label) in ["aa", "bbb", "cccc", "dd", "eeeee", "ff"]
                            .iter()
                            .enumerate()
                        {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("tag", i)))
                                    .font_size(14.0)
                                    .show(ui)
                                    .node(),
                            );
                        }
                    });
            });
        }),
        ("zstack", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("z"))
                    .size((Sizing::fixed(160.0), Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        for (i, label) in ["under", "over"].iter().enumerate() {
                            capture.push(
                                Text::new(*label)
                                    .id(WidgetId::from_hash(("layer", i)))
                                    .font_size(14.0)
                                    .show(ui)
                                    .node(),
                            );
                        }
                    });
            });
        }),
        ("canvas", |ui, capture| {
            Panel::vstack().auto_id().show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("c"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(80.0)))
                    .show(ui, |ui| {
                        for (i, label, pos) in [
                            (0, "tl", glam::Vec2::new(4.0, 4.0)),
                            (1, "br", glam::Vec2::new(80.0, 40.0)),
                        ] {
                            capture.push(
                                Text::new(label)
                                    .id(WidgetId::from_hash(("pin", i)))
                                    .position(pos)
                                    .font_size(14.0)
                                    .show(ui)
                                    .node(),
                            );
                        }
                    });
            });
        }),
    ];
    for (label, record) in cases {
        let mut h = UiHarness::with_text(UVec2::new(800, 600));
        assert_warm_rects_match_cold(
            &mut h,
            UVec2::new(800, 600),
            &format!("case: {label}"),
            *record,
        );
    }
}

/// Cache-correctness generalization: a measure-cache hit must not
/// perturb ANY downstream consumer of per-frame engine state — so a
/// the full paint-call sequence a warm frame encodes must be
/// identical to a cold frame's, operation for operation.
#[test]
fn encoded_buffer_stable_across_cache_hit_boundary() {
    let record = |ui: &mut Ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(8.0)
            .gap(6.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("transformed"))
                    .transform(TranslateScale::new(glam::Vec2::new(4.0, 2.0), 1.0))
                    .clip_rect()
                    .size((Sizing::FILL, Sizing::HUG))
                    .padding(6.0)
                    .background(Background {
                        fill: RgbaF32::srgb(0.16, 0.18, 0.22).into(),
                        border: Stroke::new(RgbaF32::srgb(0.3, 0.34, 0.42), 1.0),
                        corners: Corners::all(4.0),
                        shadow: Shadow::NONE,
                    })
                    .show(ui, |ui| {
                        Grid::new()
                            .id(WidgetId::from_hash("grid"))
                            .size((Sizing::FILL, Sizing::HUG))
                            .cols([Track::HUG, Track::FILL])
                            .rows([Track::HUG, Track::HUG])
                            .line_gap(6.0)
                            .gap(8.0)
                            .show(ui, |ui| {
                                Text::new("Title:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui);
                                Text::new(
                                    "The quick brown fox jumps over the lazy dog. \
                                     Pack my box with five dozen liquor jugs.",
                                )
                                .auto_id()
                                .font_size(14.0)
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .grid_cell((0, 1))
                                .show(ui);
                                Text::new("Tag:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((1, 0))
                                    .show(ui);
                                Text::new("layout, grid, intrinsic, wrapping")
                                    .auto_id()
                                    .font_size(14.0)
                                    .text_wrap(TextWrap::WrapWithOverflow)
                                    .grid_cell((1, 1))
                                    .show(ui);
                            });
                    });
                Block::new()
                    .id(WidgetId::from_hash("under"))
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .background(Background::fill(RgbaF32::srgb(0.4, 0.4, 0.5)))
                    .show(ui);
            });
    };

    let mut h = UiHarness::with_text(UVec2::new(800, 600));
    h.frame(|ui| record(ui));
    let cold = h.encode_paint();

    h.frame(|ui| record(ui));
    let warm = h.encode_paint();

    assert_same_capture(&cold, &warm);
}

/// Stress test: surface resizes on both axes force the cache through
/// repeated hit/replace transitions. At each step, the warm cache's
/// rects and text shapes must equal what a cold remeasure produces —
/// clearing the measure cache is the ground-truth oracle.
///
/// Each child of the root is offered the whole surface, and each is a
/// shape whose range the cache must get right: Hug content that holds
/// past its offer, a Hug scroll whose cap binds below 400 px of rows, a
/// wrap stack that breaks below 900 px, a Fill share, text bound to its
/// width, a Fill canvas whose child's range moves out by its position —
/// 400 px of rows 30 px down hold from 430, so 420 must miss, and the
/// canvas's own Fill axis does not cover that for it — a grid whose text
/// wraps at the narrow widths alone, a truncating label 700 px long that
/// is cut below 700 and holds from 700 above it — Fill across, so its
/// own axis does not cover that for it — and a Hug grid whose two
/// 350 px Hug columns are squeezed below 700 and hold from 700 above it.
/// A Fill hstack shares its width the same way before its two 350 px
/// labels measure, and a vstack shares its height at arrange between a
/// header and a scroll below 430.
/// The sizes step both ways across each of those thresholds. Labels that
/// fit — the Hug stack of buttons — hold under every surface, as fixed
/// blocks do.
#[test]
fn cache_rects_match_cold_oracle_across_resizes() {
    // 100 cells of the mono metric's 7 px at 14 px: 700 px on one line.
    const LONG_LABEL: &str = "0123456789012345678901234567890123456789\
                              0123456789012345678901234567890123456789\
                              01234567890123456789";
    let rows = |ui: &mut Ui, salt: &'static str| {
        for i in 0..8u32 {
            Block::new()
                .id_salt((salt, i))
                .size((Sizing::fixed(120.0), Sizing::fixed(50.0)))
                .show(ui);
        }
    };
    let record = |ui: &mut Ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("xform"))
                    .transform(TranslateScale::new(glam::Vec2::new(2.0, 2.0), 1.0))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Grid::new()
                            .id(WidgetId::from_hash("g"))
                            .size((Sizing::FILL, Sizing::HUG))
                            .cols([Track::HUG, Track::FILL])
                            .rows([Track::HUG])
                            .show(ui, |ui| {
                                Text::new("Title:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui);
                                Text::new(
                                    "Lorem ipsum dolor sit amet, consectetur \
                                     adipiscing elit, sed do eiusmod tempor \
                                     incididunt ut labore et dolore magna \
                                     aliqua. Ut enim ad minim veniam.",
                                )
                                .auto_id()
                                .font_size(14.0)
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .grid_cell((0, 1))
                                .show(ui);
                            });
                    });
                Panel::vstack()
                    .id(WidgetId::from_hash("blocks"))
                    .padding(4.0)
                    .show(ui, |ui| {
                        for i in 0..3u32 {
                            Block::new()
                                .id_salt(("block", i))
                                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                                .show(ui);
                        }
                    });
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .show(ui, |ui| rows(ui, "scroll-row"));
                Panel::wrap_hstack()
                    .id(WidgetId::from_hash("wrap"))
                    .show(ui, |ui| {
                        for i in 0..6u32 {
                            Block::new()
                                .id_salt(("wrap-item", i))
                                .size((Sizing::fixed(150.0), Sizing::fixed(20.0)))
                                .show(ui);
                        }
                    });
                Panel::vstack()
                    .id(WidgetId::from_hash("fill"))
                    .show(ui, |ui| {
                        Block::new()
                            .id_salt("fill-share")
                            .size((Sizing::fixed(80.0), Sizing::FILL))
                            .show(ui);
                        Block::new()
                            .id_salt("fill-fixed")
                            .size((Sizing::fixed(80.0), Sizing::fixed(30.0)))
                            .show(ui);
                    });
                Panel::vstack()
                    .id(WidgetId::from_hash("paragraph"))
                    .show(ui, |ui| {
                        Text::new(
                            "Sed ut perspiciatis unde omnis iste natus error sit \
                             voluptatem accusantium doloremque laudantium.",
                        )
                        .auto_id()
                        .font_size(14.0)
                        .text_wrap(TextWrap::Wrap)
                        .show(ui);
                    });
                Panel::hstack()
                    .id(WidgetId::from_hash("buttons"))
                    .show(ui, |ui| {
                        Button::new().id_salt("ok").label("OK").show(ui);
                        Button::new().id_salt("cancel").label("Cancel").show(ui);
                    });
                Panel::vstack()
                    .id(WidgetId::from_hash("label"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Text::new(LONG_LABEL)
                            .auto_id()
                            .size((Sizing::FILL, Sizing::HUG))
                            .font_size(14.0)
                            .text_wrap(TextWrap::Truncate)
                            .show(ui);
                    });
                Grid::new()
                    .id(WidgetId::from_hash("hug-grid"))
                    .cols([Track::HUG, Track::HUG])
                    .rows([Track::HUG])
                    .show(ui, |ui| {
                        for col in 0..2u16 {
                            Text::new(&LONG_LABEL[..50])
                                .id_salt(("hug-grid-cell", col))
                                .font_size(14.0)
                                .text_wrap(TextWrap::Truncate)
                                .grid_cell((0, col))
                                .show(ui);
                        }
                    });
                Panel::zstack()
                    .id(WidgetId::from_hash("canvas-wrap"))
                    .show(ui, |ui| {
                        Panel::canvas()
                            .id(WidgetId::from_hash("canvas"))
                            .size((Sizing::HUG, Sizing::FILL))
                            .show(ui, |ui| {
                                Scroll::vertical()
                                    .id(WidgetId::from_hash("canvas-scroll"))
                                    .position((20.0, 30.0))
                                    .show(ui, |ui| rows(ui, "canvas-row"));
                            });
                        Panel::hstack()
                            .id(WidgetId::from_hash("width-share"))
                            .size((Sizing::FILL, Sizing::HUG))
                            .show(ui, |ui| {
                                for col in 0..2u16 {
                                    Text::new(&LONG_LABEL[..50])
                                        .id_salt(("width-share-cell", col))
                                        .font_size(14.0)
                                        .text_wrap(TextWrap::Truncate)
                                        .show(ui);
                                }
                            });
                        Panel::vstack()
                            .id(WidgetId::from_hash("height-share"))
                            .show(ui, |ui| {
                                Block::new()
                                    .id_salt("height-share-header")
                                    .size((Sizing::fixed(80.0), Sizing::fixed(30.0)))
                                    .show(ui);
                                Scroll::vertical()
                                    .id(WidgetId::from_hash("height-share-scroll"))
                                    .show(ui, |ui| rows(ui, "height-share-row"));
                            });
                    });
            });
    };
    let rects = |h: &UiHarness| -> Vec<_> {
        (0..h.ui.tree(Layer::Main).records.len() as u32)
            .map(|i| h.ui.arranged_rect(Layer::Main, NodeId(i)))
            .collect()
    };
    // Rects alone can hide a stale measure: arrange places a stale run
    // at the right rect while it paints the text it was shaped to.
    let shapes = |h: &UiHarness| -> Vec<_> {
        h.ui.layout(Layer::Main)
            .text_shapes
            .iter()
            .map(|shaped| (shaped.extent, shaped.key))
            .collect()
    };

    let sizes = [
        (800, 600),
        (800, 700),
        (800, 500),
        (600, 600),
        (1000, 350),
        (700, 450),
        (1000, 650),
        (800, 420),
        (800, 600),
        (600, 380),
        (900, 900),
    ];
    let mut h = UiHarness::new(UVec2::new(sizes[0].0, sizes[0].1));
    for (i, &(w, ht)) in sizes.iter().enumerate() {
        h.resize(UVec2::new(w, ht));
        h.frame(record);
        let warm = rects(&h);
        let warm_shapes = shapes(&h);
        if i > 0 {
            for held in ["blocks", "buttons"] {
                assert!(
                    h.engines
                        .layout
                        .scratch
                        .counters
                        .cache_hits()
                        .contains(&WidgetId::from_hash(held)),
                    "step {i}: `{held}` holds under any surface past its content",
                );
            }
        }

        h.engines.layout.cache.forget_all();
        h.frame(record);
        assert_eq!(
            warm,
            rects(&h),
            "step {i}: warm-cache rects diverged from cold remeasure at {w}x{ht}",
        );
        assert_eq!(
            warm_shapes,
            shapes(&h),
            "step {i}: warm-cache text shapes diverged from cold remeasure at {w}x{ht}",
        );
    }
}

/// Registering a font invalidates the measure-cache snapshot, which no
/// check inside the cache can reach on its own.
///
/// Every freshness test here asks whether the *inputs* moved — the
/// subtree hash, the quantized available width — and a load moves
/// neither while changing what every run in the tree measures to. A
/// family that fell back to the bundled default keeps the same
/// `TextShapeKey` once its own face arrives, so without the epoch check
/// in `LayoutEngine::run` the snapshot replays widths measured against
/// the old database for as long as the tree is unchanged, while the
/// renderer paints the new face inside them.
///
/// Pinned on the shaper's dispatch count, which is what a *replayed*
/// snapshot leaves flat: the cache short-circuits whole subtrees, so an
/// unchanged run reaches neither `TextSystem` nor the shaper.
#[test]
fn registering_a_font_forces_the_next_frame_to_remeasure() {
    let mut h = UiHarness::with_text(UVec2::new(400, 300));
    let record = |ui: &mut Ui| {
        Text::new("a label that sizes to its own text")
            .auto_id()
            .show(ui);
    };
    let dispatches = |h: &UiHarness| h.engines.layout.text.shaper().measure_calls();

    h.prime(2, record);
    let warm = dispatches(&h);
    h.frame(record);
    assert_eq!(
        dispatches(&h),
        warm,
        "premise: an unchanged tree replays the snapshot and shapes nothing",
    );

    h.ui.load_font(INTER).expect("the bundled Inter loads");
    h.frame(record);
    assert!(
        dispatches(&h) > warm,
        "a font load must throw the snapshot away and remeasure",
    );
}

/// O1 regression: a measure-cache hit restores the subtree root's
/// intrinsics, so when a deep sibling changes and forces the ancestor
/// chain to re-measure, the ancestor's `IntrinsicQuery::children_max` reads the
/// unchanged sibling's cached intrinsic instead of cold-recursing through
/// its whole subtree (which would re-probe the text cache per leaf).
/// Pinned via the per-frame `intrinsic_computes` counter.
#[test]
fn measure_cache_restores_intrinsics_so_localized_change_skips_sibling_rewalk() {
    const HEAVY: usize = 30;

    fn build(ui: &mut Ui, tick: u32) {
        Panel::vstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                // Unchanging heavy subtree: many text leaves.
                Panel::vstack()
                    .id_salt("heavy")
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        for i in 0..HEAVY {
                            Text::new("lorem ipsum dolor").id_salt(("h", i)).show(ui);
                        }
                    });
                // Tiny sibling whose text changes each frame. Constant
                // width under the mono test shaper, so layout is stable
                // and `heavy` stays a cache hit.
                let label = ui.fmt(format_args!("tick {tick:04}"));
                Text::new(label).id_salt("tiny").show(ui);
            });
    }

    let size = UVec2::new(400, 600);
    let mut h = UiHarness::new(size);

    // Cold frame computes intrinsics across the whole tree.
    h.frame(|ui| build(ui, 0));
    let cold = h.engines.layout.scratch.counters.intrinsic_computes() as usize;
    assert!(
        cold > HEAVY,
        "cold frame should compute the whole tree's intrinsics, got {cold}",
    );

    // Warm frame: only `tiny` changes. `heavy` hits the cache; its
    // restored intrinsics keep the root re-measure from re-walking it, so
    // the count collapses to the changed ancestor chain (~root + tiny),
    // not ~2·HEAVY for a full sibling re-walk.
    h.frame(|ui| build(ui, 1));
    let warm = h.engines.layout.scratch.counters.intrinsic_computes() as usize;
    assert!(
        warm < HEAVY / 2,
        "localized change re-walked the unchanged sibling: {warm} intrinsic \
         computes (heavy={HEAVY}, cold={cold}); the cache-hit intrinsic restore \
         should bound this to the changed ancestor chain",
    );
}

/// A measure-cache hit hands its subtree root's floor back to the parent
/// that re-measures around it, as a cold measure would.
///
/// Shape: a Hug zstack under a 50 px bound holds a wrapped paragraph
/// panel, the hit, and a label that changes each frame, which forces the
/// zstack to re-measure. The paragraph wraps to three 14 px lines in 344
/// px, 51 px tall (`cross_driver_tests::support::lines_h(3, 14.0)`), so the zstack is floored at 51 rather than capped at
/// the 50 its parent offers. Read from a hit that dropped the floor, it
/// would take the 50.
#[test]
fn measure_cache_hit_restores_the_floor_its_parent_reads() {
    let build = |ui: &mut Ui, tick: u32| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(360.0), Sizing::fixed(50.0)))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("mid"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        Panel::vstack()
                            .id(WidgetId::from_hash("paragraph"))
                            .size((Sizing::fixed(344.0), Sizing::HUG))
                            .show(ui, |ui| {
                                Text::new(
                                    "The quick brown fox jumps over the lazy dog. \
                                     Pack my box with five dozen liquor jugs. \
                                     How vexingly quick daft zebras jump!",
                                )
                                .auto_id()
                                .font_size(14.0)
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .show(ui);
                            });
                        let label = ui.fmt(format_args!("tick {tick}"));
                        Text::new(label).id_salt("label").show(ui);
                    });
            });
    };

    let mut h = UiHarness::with_text(UVec2::new(800, 600));
    for tick in 0..2 {
        h.frame(|ui| build(ui, tick));
        assert_eq!(
            h.arranged(WidgetId::from_hash("mid")).size.h,
            51.0,
            "tick {tick}"
        );
    }
    let hits = h.engines.layout.scratch.counters.cache_hits();
    assert!(
        hits.contains(&WidgetId::from_hash("paragraph"))
            && !hits.contains(&WidgetId::from_hash("mid")),
        "the paragraph hits and its parent re-measures: {hits:?}",
    );
}

/// A subtree whose slot **moves without resizing** replays its rects
/// translated rather than re-running the drivers
/// (`LayoutEngine::replay_arranged`). This is the only replay branch that
/// rewrites values instead of copying them verbatim, so it gets three
/// independent assertions: the branch actually fired, the shift is exactly
/// the header's growth on Y and zero on X, and the result still equals a
/// cold remeasure.
///
/// Shape: a vstack whose fixed-height header grows, followed by an
/// untouched nested subtree. Every node below the header shifts by the
/// growth with its size intact — the "a sibling above grew, so everything
/// below shifts by dy" case.
#[test]
fn moved_subtree_replays_translated_rects() {
    const ROWS: usize = 4;
    let record = |ui: &mut Ui, header_h: f32, capture: &mut Vec<NodeId>| {
        capture.clear();
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("header"))
                    .size((Sizing::FILL, Sizing::fixed(header_h)))
                    .show(ui, |_ui| {});
                Panel::vstack()
                    .id(WidgetId::from_hash("stable"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        for row in 0..ROWS {
                            let outer = Panel::hstack()
                                .id(WidgetId::from_hash(("row", row)))
                                .size((Sizing::FILL, Sizing::fixed(20.0)))
                                .show(ui, |ui| {
                                    capture.push(
                                        Panel::zstack()
                                            .id(WidgetId::from_hash(("cell", row)))
                                            .size((Sizing::fixed(30.0), Sizing::FILL))
                                            .show(ui, |_ui| {})
                                            .response
                                            .node(),
                                    );
                                });
                            capture.push(outer.response.node());
                        }
                    });
            });
    };

    let size = UVec2::new(800, 600);
    let rects = |ui: &Ui, nodes: &[NodeId]| -> Vec<_> {
        nodes
            .iter()
            .map(|&n| ui.arranged_rect(Layer::Main, n))
            .collect()
    };

    let mut h = UiHarness::new(size);
    let mut before_nodes = Vec::new();
    h.frame(|ui| record(ui, 10.0, &mut before_nodes));
    let before = rects(&h.ui, &before_nodes);

    let mut after_nodes = Vec::new();
    h.frame(|ui| record(ui, 30.0, &mut after_nodes));
    let after = rects(&h.ui, &after_nodes);

    // Non-vacuity: the translate branch must be the one that ran. Without
    // this the test still passes if arrange re-derived every rect.
    assert!(
        h.engines
            .layout
            .scratch
            .counters
            .arrange_replays()
            .translated
            > 0,
        "no subtree replayed via translation — fixture pins nothing, got {:?}",
        h.engines.layout.scratch.counters.arrange_replays(),
    );

    // The header grew 10 → 30, so everything below shifts down exactly 20
    // and keeps its size. Hand-computed, not a range check.
    assert_eq!(before.len(), ROWS * 2);
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        assert_eq!(a.size, b.size, "node {i} resized during a pure translation");
        assert_eq!(a.min.x, b.min.x, "node {i} drifted on X");
        assert_eq!(a.min.y, b.min.y + 20.0, "node {i} shifted by the wrong dy");
    }

    // Ground truth: clearing the cache forces a full remeasure of the same
    // frame, which must land on the identical geometry.
    h.engines.layout.cache.forget_all();
    let mut cold_nodes = Vec::new();
    h.frame(|ui| record(ui, 30.0, &mut cold_nodes));
    assert_eq!(
        after,
        rects(&h.ui, &cold_nodes),
        "translated replay diverged from a cold remeasure",
    );
}

/// A hit subtree whose root is arranged at a new size still replays the
/// descendants that keep theirs. The `stable` panel fills a Hug ZStack
/// whose width follows a sibling: measured against the ZStack's constant
/// offer, it hits the cache, but it is arranged at the grown width, so its
/// own driver runs. Its rows are a fixed 60×20 at the top-left of it and
/// arranged unchanged, so each replays its cached rects instead of
/// dispatching.
#[test]
fn a_resized_hit_root_replays_its_unchanged_descendants() {
    const ROWS: usize = 3;
    let record = |ui: &mut Ui, grower_w: f32, capture: &mut Vec<NodeId>| {
        capture.clear();
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("host"))
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        Panel::zstack()
                            .id(WidgetId::from_hash("grower"))
                            .size((Sizing::fixed(grower_w), Sizing::fixed(80.0)))
                            .show(ui, |_ui| {});
                        Panel::vstack()
                            .id(WidgetId::from_hash("stable"))
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                for row in 0..ROWS {
                                    let outer = Panel::hstack()
                                        .id(WidgetId::from_hash(("row", row)))
                                        .size((Sizing::fixed(60.0), Sizing::fixed(20.0)))
                                        .show(ui, |ui| {
                                            capture.push(
                                                Panel::zstack()
                                                    .id(WidgetId::from_hash(("cell", row)))
                                                    .size((Sizing::fixed(30.0), Sizing::FILL))
                                                    .show(ui, |_ui| {})
                                                    .response
                                                    .node(),
                                            );
                                        });
                                    capture.push(outer.response.node());
                                }
                            });
                    });
            });
    };
    let rects = |ui: &Ui, nodes: &[NodeId]| -> Vec<_> {
        nodes
            .iter()
            .map(|&n| ui.arranged_rect(Layer::Main, n))
            .collect()
    };

    let mut h = UiHarness::new(UVec2::new(800, 600));
    let mut nodes = Vec::new();
    h.frame(|ui| record(ui, 100.0, &mut nodes));
    h.frame(|ui| record(ui, 150.0, &mut nodes));
    assert!(
        h.engines
            .layout
            .scratch
            .counters
            .cache_hits()
            .contains(&WidgetId::from_hash("stable")),
        "premise: the stable panel's measure hits the cache",
    );
    assert_eq!(
        h.engines.layout.scratch.counters.arrange_replays().copied,
        ROWS as u32,
        "each row, arranged where it was, replays",
    );
    let warm = rects(&h.ui, &nodes);

    h.engines.layout.cache.forget_all();
    h.frame(|ui| record(ui, 150.0, &mut nodes));
    assert_eq!(
        warm,
        rects(&h.ui, &nodes),
        "replay diverged from a cold remeasure"
    );
}
