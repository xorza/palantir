//! Per-frame aggregate benchmark. Builds a synthetic but realistic UI tree
//! exercising every public layout driver (HStack/VStack/ZStack/Canvas/Grid/
//! WrapStack/Scroll), every public widget (Panel/Frame/Button/Text/Grid/
//! Scroll/Checkbox/RadioButton/TextEdit/Tooltip/Popup), every `Shape`
//! variant (RoundedRect / Line / Polyline / CubicBezier / QuadraticBezier
//! / Mesh / Text), every `Brush` variant (Solid / Linear / Radial /
//! Conic), and the popup/tooltip layers. Drives all five passes
//! (record/measure/arrange/cascade/encode+compose) every iteration.
//!
//! Uses `TextShaper::with_bundled_fonts()` so text measurement reflects
//! realistic cosmic-text shaping cost (not the mono fallback). This is
//! what real apps pay per frame.
//!
//! Two arms — both run the same workload, differ only in cache state:
//!
//! - **`frame/cached`** — viewport size is fixed, so the `MeasureCache`
//!   key `(WidgetId, subtree_hash, available_q)` stays stable across
//!   iterations and the cache hits at the highest stable root every
//!   frame. Steady-state cost of the pipeline with warm caches.
//! - **`frame/resizing`** — viewport size mutates every iteration. The
//!   `available_q` quantization changes per frame, every measure-cache
//!   key misses, and measure rebuilds from scratch. Approximates a live
//!   drag-resize, and also the "uncached" baseline since the ungated
//!   `Ui` surface has no `clear_measure_cache`.
//!
//! Ratio of `cached / resizing` quantifies what the measure cache buys
//! on the realistic workload. See `caches.rs` for finer per-axis
//! cache A/B benches (gated behind `bench-deep`).

use criterion::{Criterion, criterion_group, criterion_main};
use palantir::renderer::frontend::Frontend;
use palantir::ui::frame_report::RenderPlan;
use palantir::{
    Align, Background, Brush, Button, Checkbox, Color, ColorU8, Configure, ConicGradient, Corners,
    Display, Frame, FrameArena, FrameStamp, Grid, Justify, LineCap, LineJoin, LinearGradient, Mesh,
    Panel, PolylineColors, Popup, RadialGradient, RadioButton, Rect, RenderCaches, Scroll, Shadow,
    Shape, Sizing, Stop, Stroke, Text, TextEdit, TextShaper, TextStyle, Tooltip, Track, Ui,
};
use std::hint::black_box;
use std::rc::Rc;

const WORKLOAD_SCALE: usize = 32;

/// Persistent state for widgets that mutate user data (TextEdit needs
/// a `&mut String`, Checkbox a `&mut bool`, RadioButton a `&mut T`).
/// Threaded through `build_ui` so the bench can construct it once and
/// reuse across frames without per-iter allocation.
#[derive(Default)]
struct FormState {
    name: String,
    notes: String,
    enabled: bool,
    role: u8,
}

fn build_ui(state: &mut FormState, ui: &mut Ui) {
    let sidebar_items = 5 * WORKLOAD_SCALE;
    let chat_messages = 2 * WORKLOAD_SCALE;
    let canvas_dots = 3 * WORKLOAD_SCALE;
    let prop_rows = 4 + WORKLOAD_SCALE;
    let tag_count = 3 * WORKLOAD_SCALE;

    let panel_bg = Background {
        fill: Color::rgb(0.14, 0.16, 0.22).into(),
        stroke: Stroke::solid(Color::rgb(0.28, 0.34, 0.46), 1.0),
        corners: Corners::all(6.0),
        shadow: Shadow::NONE,
    };

    Panel::vstack()
        .gap(8.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .background(panel_bg.clone())
        .show(ui, |ui| {
            // ── Header bar: real cosmic-shaped title + Fill spacer +
            // action buttons. Exercises stack Fill + child_align.
            Panel::hstack()
                .gap(8.0)
                .size((Sizing::FILL, Sizing::Hug))
                .child_align(Align::CENTER)
                .show(ui, |ui| {
                    Text::new("Palantir — Frame Bench")
                        .id_salt("title")
                        .style(TextStyle::default().with_font_size(20.0))
                        .show(ui);
                    Frame::new()
                        .id_salt("title-spacer")
                        .size((Sizing::FILL, Sizing::Fixed(1.0)))
                        .show(ui);
                    for i in 0..5 {
                        let label = ui.fmt(format_args!("Action {i}"));
                        let btn = Button::new().id_salt(("hdr", i)).label(label).show(ui);
                        // Tooltip layer: anchored to the button's response.
                        // Exercises `Layer::Tooltip` recording.
                        Tooltip::for_(&btn)
                            .text("Header action")
                            .delay(0.0)
                            .show(ui);
                    }
                });

            // ── Body: HStack with Fixed sidebar + Fill main column.
            Panel::hstack()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // ── Sidebar: Scrolled VStack of fill-width buttons +
                    // justify-center sub-stack. Exercises Scroll + nested
                    // stacks at depth.
                    Panel::vstack()
                        .gap(4.0)
                        .padding(8.0)
                        .size((Sizing::Fixed(220.0), Sizing::FILL))
                        .background(panel_bg.clone())
                        .clip_rounded()
                        .show(ui, |ui| {
                            Scroll::vertical()
                                .id_salt("sidebar-scroll")
                                .gap(4.0)
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| {
                                    for i in 0..sidebar_items {
                                        let label = ui.fmt(format_args!("Sidebar item {i}"));
                                        Button::new()
                                            .id_salt(("side", i))
                                            .label(label)
                                            .size((Sizing::FILL, Sizing::Hug))
                                            .show(ui);
                                    }
                                });
                            Frame::new()
                                .id_salt("sb-divider")
                                .size((Sizing::FILL, Sizing::Fixed(1.0)))
                                .margin(4.0)
                                .show(ui);
                            Panel::hstack()
                                .gap(2.0)
                                .justify(Justify::Center)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    for i in 0..3 {
                                        Button::new()
                                            .id_salt(("sb-foot", i))
                                            .label(ui.fmt(format_args!("F{i}")))
                                            .show(ui);
                                    }
                                });
                        });

                    // ── Main column.
                    Panel::vstack()
                        .gap(10.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            // Property grid — Hug label col + Fill value col
                            // with wrapping text. Exercises Grid + intrinsic.
                            let rows: Vec<Track> = (0..prop_rows).map(|_| Track::hug()).collect();
                            Grid::new()
                                .id_salt("props")
                                .cols(Rc::from([
                                    Track::hug().min(80.0),
                                    Track::fill(),
                                    Track::fixed(60.0),
                                ]))
                                .rows(Rc::<[Track]>::from(rows))
                                .gap(6.0)
                                .padding(4.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui, |ui| {
                                    let labels = [
                                        "Name",
                                        "Description",
                                        "Author",
                                        "License",
                                        "Created",
                                        "Modified",
                                        "Tags",
                                        "Notes",
                                    ];
                                    let values = [
                                        "the quick brown fox jumps over the lazy dog",
                                        "Lorem ipsum dolor sit amet consectetur adipiscing elit \
                                         sed do eiusmod tempor",
                                        "Jane Doe and a long author name to force wrapping",
                                        "MIT-or-Apache-2.0",
                                    ];
                                    for row in 0..prop_rows {
                                        let r = row as u16;
                                        Text::new(labels[row % labels.len()])
                                            .id_salt(("plbl", row))
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .grid_cell((r, 0))
                                            .show(ui);
                                        Text::new(values[row % values.len()])
                                            .id_salt(("pval", row))
                                            .style(TextStyle::default().with_font_size(14.0))
                                            .wrapping()
                                            .grid_cell((r, 1))
                                            .show(ui);
                                        Button::new()
                                            .id_salt(("pact", row))
                                            .label("Edit")
                                            .grid_cell((r, 2))
                                            .show(ui);
                                    }
                                });

                            // ── Form row: TextEdit + Checkbox + Radio
                            // group + Button. Exercises stateful widgets
                            // that read `&mut` user storage.
                            Panel::hstack()
                                .id_salt("form-row")
                                .gap(8.0)
                                .padding(6.0)
                                .child_align(Align::CENTER)
                                .size((Sizing::FILL, Sizing::Hug))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    TextEdit::new(&mut state.name)
                                        .id_salt("edit-name")
                                        .size((Sizing::Fill(2.0), Sizing::Hug))
                                        .show(ui);
                                    Checkbox::new(&mut state.enabled)
                                        .id_salt("enabled")
                                        .label("enabled")
                                        .show(ui);
                                    for v in 0u8..3 {
                                        RadioButton::new(&mut state.role, v)
                                            .id_salt(("role", v))
                                            .label(["read", "write", "admin"][v as usize])
                                            .show(ui);
                                    }
                                    Button::new().id_salt("submit").label("Submit").show(ui);
                                });

                            // ── Tag wrap list: WrapHStack flowing many
                            // small buttons across multiple lines.
                            // Exercises wrapstack measure/arrange.
                            Panel::wrap_hstack()
                                .id_salt("tags")
                                .gap(4.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::Hug))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    for i in 0..tag_count {
                                        let label = ui.fmt(format_args!("#tag{i}"));
                                        Button::new().id_salt(("tag", i)).label(label).show(ui);
                                    }
                                });

                            // ── Shape gallery: every Shape variant + every
                            // Brush variant, all in one Canvas. Exercises
                            // encode + compose on a heterogeneous mix.
                            Panel::canvas()
                                .id_salt("shape-gallery")
                                .size((Sizing::FILL, Sizing::Fixed(140.0)))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    add_shape_gallery(ui);
                                });

                            // ── Chat list inside a Scroll. Wrapping text
                            // + Fill column inside an HStack with Fixed
                            // avatar. Scroll wraps so this is also the
                            // big inner scroll case.
                            Scroll::vertical()
                                .id_salt("chat-scroll")
                                .gap(6.0)
                                .padding(4.0)
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| {
                                    for i in 0..chat_messages {
                                        Panel::hstack()
                                            .id_salt(("chat-row", i))
                                            .gap(8.0)
                                            .size((Sizing::FILL, Sizing::Hug))
                                            .show(ui, |ui| {
                                                Frame::new()
                                                    .id_salt(("avatar", i))
                                                    .size((
                                                        Sizing::Fixed(40.0),
                                                        Sizing::Fixed(40.0),
                                                    ))
                                                    .background(Background {
                                                        fill: Brush::Radial(
                                                            RadialGradient::two_stop_centered(
                                                                ColorU8::hex(0xfacc15),
                                                                ColorU8::hex(0x4c5cdb),
                                                            ),
                                                        ),
                                                        corners: Corners::all(20.0),
                                                        ..Default::default()
                                                    })
                                                    .show(ui);
                                                Panel::vstack()
                                                    .id_salt(("chat-text", i))
                                                    .gap(2.0)
                                                    .size((Sizing::FILL, Sizing::Hug))
                                                    .show(ui, |ui| {
                                                        let name = ui.fmt(format_args!("user_{i}"));
                                                        Text::new(name)
                                                            .id_salt(("from", i))
                                                            .style(
                                                                TextStyle::default()
                                                                    .with_font_size(12.0),
                                                            )
                                                            .show(ui);
                                                        Text::new(
                                                            "Longer body that should wrap inside \
                                                             the Fill column without breaking \
                                                             words inside any single token.",
                                                        )
                                                        .id_salt(("msg", i))
                                                        .style(
                                                            TextStyle::default()
                                                                .with_font_size(13.0),
                                                        )
                                                        .wrapping()
                                                        .size((Sizing::FILL, Sizing::Hug))
                                                        .show(ui);
                                                    });
                                            });
                                    }
                                });

                            // ── Decorative canvas (existing): many small
                            // absolutely-positioned dots.
                            Panel::canvas()
                                .id_salt("dot-canvas")
                                .size((Sizing::FILL, Sizing::Fixed(80.0)))
                                .show(ui, |ui| {
                                    for i in 0..canvas_dots {
                                        Frame::new()
                                            .id_salt(("dot", i))
                                            .size((Sizing::Fixed(16.0), Sizing::Fixed(16.0)))
                                            .position((
                                                i as f32 * 22.0,
                                                12.0 + (i % 3) as f32 * 18.0,
                                            ))
                                            .background(Background {
                                                fill: Color::rgb(0.32, 0.46, 0.66).into(),
                                                corners: Corners::all(8.0),
                                                ..Default::default()
                                            })
                                            .show(ui);
                                    }
                                });

                            // ── TextEdit with multi-line notes content —
                            // exercises the wrapping text-edit branch in
                            // measure + the `Text` shape lowering.
                            TextEdit::new(&mut state.notes)
                                .id_salt("notes")
                                .size((Sizing::FILL, Sizing::Fixed(60.0)))
                                .show(ui);
                        });
                });

            // ── Footer: ZStack overlay — bg frame + centered status text
            // + a Popup pinned to the bottom-left. Exercises `Layer::Popup`.
            Panel::zstack()
                .size((Sizing::FILL, Sizing::Fixed(36.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("footer-bg")
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: Brush::Linear(LinearGradient::two_stop(
                                0.0,
                                ColorU8::hex(0x1a1a2e),
                                ColorU8::hex(0x2a2a3e),
                            )),
                            corners: Corners::all(4.0),
                            ..Default::default()
                        })
                        .show(ui);
                    Panel::hstack()
                        .padding(6.0)
                        .gap(6.0)
                        .child_align(Align::CENTER)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Text::new("Ready")
                                .id_salt("footer-status")
                                .style(TextStyle::default().with_font_size(12.0))
                                .show(ui);
                            Frame::new()
                                .id_salt("footer-spacer")
                                .size((Sizing::FILL, Sizing::Fixed(1.0)))
                                .show(ui);
                            Text::new("v1.2.3 · many nodes")
                                .id_salt("footer-meta")
                                .style(TextStyle::default().with_font_size(12.0))
                                .show(ui);
                        });

                    // Always-visible popup near the bottom-left of the
                    // surface, used here purely to exercise the popup
                    // recording layer + anchoring math.
                    Popup::anchored_to(glam::Vec2::new(20.0, 600.0))
                        .background(panel_bg.clone())
                        .show(ui, |ui, _handle| {
                            Text::new("Popup layer")
                                .id_salt("popup-label")
                                .style(TextStyle::default().with_font_size(11.0))
                                .show(ui);
                        });
                });
        });
}

/// One canvas covering every `Shape` variant and every `Brush` variant.
/// Sits inside the main column's shape-gallery panel.
fn add_shape_gallery(ui: &mut Ui) {
    // Solid + Linear-gradient RoundedRects (left strip).
    ui.add_shape(Shape::RoundedRect {
        local_rect: Some(Rect::new(4.0, 6.0, 60.0, 30.0)),
        corners: Corners::all(6.0),
        fill: Color::rgb(0.85, 0.30, 0.30).into(),
        stroke: Stroke::solid(Color::rgb(1.0, 1.0, 1.0), 1.0),
    });
    ui.add_shape(Shape::RoundedRect {
        local_rect: Some(Rect::new(4.0, 42.0, 60.0, 30.0)),
        corners: Corners::all(6.0),
        fill: Brush::Linear(LinearGradient::two_stop(
            std::f32::consts::FRAC_PI_2,
            ColorU8::hex(0x1a1a2e),
            ColorU8::hex(0x4c5cdb),
        )),
        stroke: Stroke::ZERO,
    });
    // Radial + Conic gradient RoundedRects (left strip, lower).
    ui.add_shape(Shape::RoundedRect {
        local_rect: Some(Rect::new(4.0, 78.0, 60.0, 30.0)),
        corners: Corners::all(6.0),
        fill: Brush::Radial(RadialGradient::two_stop_centered(
            ColorU8::hex(0xfacc15),
            ColorU8::hex(0x1a1a2e),
        )),
        stroke: Stroke::ZERO,
    });
    ui.add_shape(Shape::RoundedRect {
        local_rect: Some(Rect::new(70.0, 6.0, 60.0, 30.0)),
        corners: Corners::all(6.0),
        fill: Brush::Conic(ConicGradient::new(
            glam::Vec2::splat(0.5),
            0.0,
            [
                Stop::new(0.0, ColorU8::hex(0xff5e44)),
                Stop::new(0.5, ColorU8::hex(0x46c46c)),
                Stop::new(1.0, ColorU8::hex(0x4c5cdb)),
            ],
        )),
        stroke: Stroke::ZERO,
    });

    // Line + Polyline (centre strip).
    ui.add_shape(Shape::Line {
        a: glam::Vec2::new(140.0, 12.0),
        b: glam::Vec2::new(240.0, 36.0),
        width: 3.0,
        brush: Color::rgb(0.2, 0.9, 1.0).into(),
        cap: LineCap::Round,
        join: LineJoin::Miter,
    });
    let zigzag: [glam::Vec2; 5] = [
        glam::Vec2::new(140.0, 48.0),
        glam::Vec2::new(170.0, 70.0),
        glam::Vec2::new(200.0, 48.0),
        glam::Vec2::new(220.0, 70.0),
        glam::Vec2::new(240.0, 48.0),
    ];
    let zigzag_cols = [
        Color::rgb(1.0, 0.4, 0.4),
        Color::rgb(1.0, 0.85, 0.2),
        Color::rgb(0.2, 1.0, 0.4),
        Color::rgb(0.2, 0.6, 1.0),
        Color::rgb(0.8, 0.4, 1.0),
    ];
    ui.add_shape(Shape::Polyline {
        points: &zigzag,
        colors: PolylineColors::PerPoint(&zigzag_cols),
        width: 4.0,
        cap: LineCap::Butt,
        join: LineJoin::Round,
    });

    // Cubic + Quadratic bezier (right strip).
    ui.add_shape(Shape::CubicBezier {
        p0: glam::Vec2::new(250.0, 30.0),
        p1: glam::Vec2::new(280.0, -10.0),
        p2: glam::Vec2::new(340.0, -10.0),
        p3: glam::Vec2::new(370.0, 30.0),
        width: 5.0,
        brush: Color::rgb(0.4, 1.0, 0.6).into(),
        cap: LineCap::Round,
    });
    ui.add_shape(Shape::QuadraticBezier {
        p0: glam::Vec2::new(250.0, 70.0),
        p1: glam::Vec2::new(310.0, 30.0),
        p2: glam::Vec2::new(370.0, 70.0),
        width: 4.0,
        brush: Color::rgb(1.0, 0.85, 0.2).into(),
        cap: LineCap::Square,
    });

    // Triangle mesh — bottom strip. Exercises the mesh shape lowering
    // → mesh-pipeline path. `Shape::Mesh.mesh` borrows; leak a single
    // `Mesh` so the borrow is `'static` and per-frame iters don't
    // allocate. `Mesh` has interior `Cell`s so can't be in a `static`.
    use std::sync::OnceLock;
    static MESH_PTR: OnceLock<usize> = OnceLock::new();
    let mesh: &'static Mesh = unsafe {
        &*(*MESH_PTR.get_or_init(|| {
            let mut m = Mesh::new();
            let a = m.vertex(glam::Vec2::new(250.0, 130.0), ColorU8::hex(0xff5e44));
            let b = m.vertex(glam::Vec2::new(310.0, 90.0), ColorU8::hex(0xfacc15));
            let c = m.vertex(glam::Vec2::new(370.0, 130.0), ColorU8::hex(0x46c46c));
            m.triangle(a, b, c);
            Box::into_raw(Box::new(m)) as usize
        }) as *const Mesh)
    };
    ui.add_shape(Shape::Mesh {
        mesh,
        local_rect: None,
        tint: Color::WHITE.into(),
    });
}

/// New `Ui` with bundled cosmic fonts so text shaping is real, not
/// mono-fallback. `TextShaper::with_bundled_fonts()` is public —
/// this bench doesn't need the gated `Ui::for_test_text()`.
fn fresh_ui() -> Ui {
    Ui::new(
        TextShaper::with_bundled_fonts(),
        FrameArena::default(),
        RenderCaches::default(),
    )
}

fn bench_frame(c: &mut Criterion) {
    // ── Cached arm: fixed viewport. After the criterion warmup loop
    // primes the measure cache, every subsequent frame hits at the
    // highest stable subtree root. Includes the frontend (encode +
    // compose) so the measured cost reflects everything between
    // record and GPU submit.
    {
        let display = Display::from_physical(glam::UVec2::new(1280, 800), 2.0);
        let mut ui = fresh_ui();
        let mut frontend = Frontend::for_test_sharing(&ui);
        let mut state = FormState::default();
        c.bench_function("frame/cached", |b| {
            b.iter(|| {
                black_box(
                    ui.frame(FrameStamp::new(display, std::time::Duration::ZERO), |ui| {
                        build_ui(&mut state, ui)
                    }),
                );
                frontend.build_for_test(
                    &ui,
                    RenderPlan::Full {
                        clear: Color::BLACK,
                    },
                );
            });
        });
    }

    // ── Uncached arm: viewport mutates every iteration so the
    // `MeasureCache` key's `available_q` busts and the cache rebuilds
    // each frame. Approximates a live drag-resize. Same record →
    // frontend pipeline as the cached arm.
    {
        let mut ui = fresh_ui();
        let mut frontend = Frontend::for_test_sharing(&ui);
        let mut state = FormState::default();
        let mut frame = 0u32;
        c.bench_function("frame/resizing", |b| {
            b.iter(|| {
                let w = 1024 + (frame % 512);
                let h = 640 + ((frame / 7) % 320);
                frame = frame.wrapping_add(1);
                let display = Display::from_physical(glam::UVec2::new(w, h), 2.0);
                black_box(
                    ui.frame(FrameStamp::new(display, std::time::Duration::ZERO), |ui| {
                        build_ui(&mut state, ui)
                    }),
                );
                frontend.build_for_test(
                    &ui,
                    RenderPlan::Full {
                        clear: Color::BLACK,
                    },
                );
            });
        });
    }
}

criterion_group!(benches, bench_frame);
criterion_main!(benches);
