//! Cache × full-frame integration: the warm-cache frame must reproduce the cold layout (and encoded commands).

use crate::primitives::identity::widget_id::WidgetId;
use crate::text::font_scope::internals::INTER;
use crate::text::wrap::TextWrap;

use crate::TextStyle;
use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::internals::paint_capture::internals::assert_same_capture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::widget_core::configure::Configure;
use crate::widgets::{
    block::Block, button::Button, grid::Grid, panel::Panel, scroll::Scroll, text::Text,
};
use glam::UVec2;

/// Run `record` cold then warm and assert every captured node's rect matches. `record` pushes the nodes that matter into `capture`.
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

    h.engines.layout.forget_last_run();
    let warm_nodes = h.frame_value(|ui| {
        let mut nodes = Vec::new();
        record(ui, &mut nodes);
        nodes
    });
    let warm: Vec<_> = warm_nodes
        .iter()
        .map(|&n| h.ui.arranged_rect(Layer::Main, n))
        .collect();

    // Guard against going inert: if hash stability regresses and the warm frame misses everywhere, cold == warm passes vacuously.
    assert!(
        !h.engines.layout.scratch.counters.cache_hits().is_empty(),
        "warm frame produced no measure-cache hits — {msg} pins nothing",
    );
    assert_eq!(cold, warm, "{msg}");
}

/// Regression: a cache hit at a Grid (or ancestor) must repopulate `GridTrackStore`, or every cell collapses to x=0. Pins single, nested and sibling grids.
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
        // The hit must land at the viewport root so grid measure is skipped and the hug-restore path runs.
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

/// Per-driver cache-hit defense. Only Grid retains measure→arrange state (see the `LayoutScratch` cache-hit contract); a driver adding some without [`LayoutScratch::restore_after_cache_hit`] desyncs warm from cold.
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
            // Min-content floors force the freeze loop; a hit skips it, so arrange must read slots from `desired` alone.
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

/// A measure-cache hit must not perturb the paint-call sequence: warm equals cold, operation for operation.
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

/// Stress: resizes on both axes force repeated hit/replace transitions; warm rects and text shapes must equal a cold remeasure. Children probe thresholds, e.g. 400 px of rows 30 px down in a Fill canvas hold from 430, so 420 must miss.
#[test]
fn cache_rects_match_cold_oracle_across_resizes() {
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
                    });
            });
    };
    let rects = |h: &UiHarness| -> Vec<_> {
        (0..h.ui.tree(Layer::Main).records.len() as u32)
            .map(|i| h.ui.arranged_rect(Layer::Main, NodeId(i)))
            .collect()
    };
    // Rects alone can hide a stale measure: arrange places a stale run at the right rect while it paints the text it was shaped to.
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

/// Registering a font invalidates the measure-cache snapshot: it moves neither the subtree hash nor the width yet changes every measure, so `LayoutEngine::run` checks an epoch. Pinned on the shaper's dispatch count, which a replayed snapshot leaves flat.
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

/// O1 regression: a hit restores the subtree root's intrinsics, so a re-measuring ancestor reads them instead of re-walking the subtree. Pinned via `intrinsic_computes`.
#[test]
fn measure_cache_restores_intrinsics_so_localized_change_skips_sibling_rewalk() {
    const HEAVY: usize = 30;

    fn build(ui: &mut Ui, tick: u32) {
        Panel::vstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Panel::vstack()
                    .id_salt("heavy")
                    .size((Sizing::HUG, Sizing::HUG))
                    .show(ui, |ui| {
                        for i in 0..HEAVY {
                            Text::new("lorem ipsum dolor").id_salt(("h", i)).show(ui);
                        }
                    });
                // Tiny sibling whose text changes each frame at constant mono width, so layout is stable and `heavy` stays a hit.
                let label = ui.fmt(format_args!("tick {tick:04}"));
                Text::new(label).id_salt("tiny").show(ui);
            });
    }

    let size = UVec2::new(400, 600);
    let mut h = UiHarness::new(size);

    h.frame(|ui| build(ui, 0));
    let cold = h.engines.layout.scratch.counters.intrinsic_computes() as usize;
    assert!(
        cold > HEAVY,
        "cold frame should compute the whole tree's intrinsics, got {cold}",
    );

    // `heavy` hits and its restored intrinsics skip a re-walk: the count is the changed chain (~root + tiny), not ~2·HEAVY.
    h.frame(|ui| build(ui, 1));
    let warm = h.engines.layout.scratch.counters.intrinsic_computes() as usize;
    assert!(
        warm < HEAVY / 2,
        "localized change re-walked the unchanged sibling: {warm} intrinsic \
         computes (heavy={HEAVY}, cold={cold}); the cache-hit intrinsic restore \
         should bound this to the changed ancestor chain",
    );
}

/// A hit hands its subtree root's floor back to a re-measuring parent: a Hug zstack under a 50 px bound holding a 51 px wrapped paragraph (`lines_h(3, 14.0)`) must floor at 51, not 50.
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

/// A subtree whose slot moves without resizing replays its rects translated (`LayoutPass::replay_arranged`). Asserts the branch fired, the shift is exactly the header's growth on Y, and the result equals a cold remeasure.
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

    // The header grew 10 → 30, so everything below shifts down exactly 20 with its size intact. Hand-computed, not a range check.
    assert_eq!(before.len(), ROWS * 2);
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        assert_eq!(a.size, b.size, "node {i} resized during a pure translation");
        assert_eq!(a.min.x, b.min.x, "node {i} drifted on X");
        assert_eq!(a.min.y, b.min.y + 20.0, "node {i} shifted by the wrong dy");
    }

    h.engines.layout.cache.forget_all();
    let mut cold_nodes = Vec::new();
    h.frame(|ui| record(ui, 30.0, &mut cold_nodes));
    assert_eq!(
        after,
        rects(&h.ui, &cold_nodes),
        "translated replay diverged from a cold remeasure",
    );
}

/// A translated replay lands bit for bit where a cold arrange does: fractional sizes make `old + (new − old)` round differently from the cold sum, so the replay rebuilds each rect from its slot origin. Forty fractional header heights run through one harness, each held against a fresh cold arrange.
#[test]
fn translated_replay_lands_where_a_cold_arrange_does() {
    let record = |ui: &mut Ui, header_h: f32| {
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
                    .gap(0.3)
                    .padding(1.7)
                    .show(ui, |ui| {
                        for row in 0..12u32 {
                            Panel::hstack()
                                .id(WidgetId::from_hash(("row", row)))
                                .size((Sizing::FILL, Sizing::fixed(13.37)))
                                .margin((0.9, 1.3, 0.2, 0.7))
                                .padding((0.45, 0.8, 1.1, 0.25))
                                .gap(2.3)
                                .show(ui, |ui| {
                                    for cell in 0..3u32 {
                                        let visibility = if cell == 1 {
                                            Visibility::Collapsed
                                        } else {
                                            Visibility::Visible
                                        };
                                        Panel::zstack()
                                            .id(WidgetId::from_hash(("cell", row, cell)))
                                            .size((Sizing::fixed(30.7), Sizing::FILL))
                                            .margin((0.6, 1.9, 0.3, 0.4))
                                            .visibility(visibility)
                                            .show(ui, |ui| {
                                                Block::new()
                                                    .id(WidgetId::from_hash(("dot", row, cell)))
                                                    .size((Sizing::fixed(3.3), Sizing::fixed(2.1)))
                                                    .margin((0.15, 0.35, 0.0, 0.0))
                                                    .show(ui);
                                            });
                                    }
                                });
                        }
                    });
            });
    };
    let rects = |h: &UiHarness| -> Vec<_> {
        (0..h.ui.tree(Layer::Main).records.len() as u32)
            .map(|i| h.ui.arranged_rect(Layer::Main, NodeId(i)))
            .collect()
    };
    let size = UVec2::new(800, 2000);
    let mut warm = UiHarness::new(size);
    warm.frame(|ui| record(ui, 7.0));
    for frame in 0..40u32 {
        let header_h = 0.37 + frame as f32 * 29.13;
        warm.frame(|ui| record(ui, header_h));
        assert!(
            warm.engines
                .layout
                .scratch
                .counters
                .arrange_replays()
                .translated
                > 0,
            "frame {frame}: the subtree did not replay translated",
        );
        let mut cold = UiHarness::new(size);
        cold.frame(|ui| record(ui, header_h));
        assert_eq!(
            rects(&warm),
            rects(&cold),
            "frame {frame}, header {header_h}"
        );
    }
}

/// A hit subtree whose root is arranged at a new size still replays descendants that keep theirs: the `stable` panel hits at a constant offer but is arranged wider, and its unchanged rows replay cached rects.
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
