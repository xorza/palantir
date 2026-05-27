//! Shared `build_ui` for the `frame` bench and the `frame_visual`
//! example. A synthetic but realistic UI tree exercising every public
//! layout driver (HStack/VStack/ZStack/Canvas/Grid/WrapStack/Scroll),
//! every public widget (Panel/Frame/Button/Text/Grid/Scroll/Checkbox/
//! RadioButton/TextEdit/Tooltip/Popup), every `Shape` variant
//! (RoundedRect / Line / Polyline / CubicBezier / QuadraticBezier /
//! Mesh / Text), every `Brush` variant (Solid / Linear / Radial /
//! Conic), and the popup/tooltip layers.
//!
//! Pulled into both targets via `#[path]` so the bench measures the
//! same workload a human can eyeball through the visual example.

use std::cell::OnceCell;
use std::rc::Rc;

use palantir::{
    Align, Background, Brush, Button, Checkbox, Color, ColorU8, Configure, ConicGradient, Corners,
    Frame, Grid, Justify, LineCap, LineJoin, LinearGradient, Mesh, Panel, PolylineColors, Popup,
    RadialGradient, RadioButton, Rect, Scroll, Shadow, Shape, Sizing, Stop, Stroke, Text, TextEdit,
    TextStyle, TextWrap, Tooltip, Track, Ui,
};

// Each include site (bench / example) only uses one constant; the other
// looks dead via #[path] inclusion.
#[allow(dead_code)]
pub const BENCH_SCALE: usize = 32;
#[allow(dead_code)]
pub const VISUAL_SCALE: usize = 6;

/// Persistent state for widgets that mutate user data (TextEdit needs
/// a `&mut String`, Checkbox a `&mut bool`, RadioButton a `&mut T`).
///
/// `tick` drives the footer-status counter and is the **only** field
/// the partial-damage arm mutates between iterations. The footer Text
/// node is sized `Fixed(120.0)` so the changing digits don't shift
/// sibling layout — the damage rect collapses to that single node's
/// arranged box.
#[derive(Default)]
pub struct FormState {
    pub name: String,
    pub notes: String,
    pub enabled: bool,
    pub role: u8,
    pub tick: u32,
    /// Post-arrange translate applied to the main content panel. Used
    /// by the `frame/scrolling_cpu` bench arm to model continuous
    /// position change WITHOUT changing layout — the cascade walks the
    /// full subtree but layout/measure cache hits trivially. Tests
    /// whether a cascade delta-cache (cached output translated by
    /// `parent_transform`) would meaningfully reduce cascade cost.
    pub scroll_offset: glam::Vec2,
}

pub fn build_ui(state: &mut FormState, scale: usize, ui: &mut Ui) {
    let sidebar_items = 5 * scale;
    let chat_messages = 2 * scale;
    let canvas_dots = 3 * scale;
    let prop_rows = 4 + scale;
    let tag_count = 3 * scale;

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
                        let btn = Button::new()
                            .id_salt(("hdr", i))
                            .label(label)
                            .show(ui)
                            .snapshot();
                        Tooltip::for_(&btn)
                            .text("Header action")
                            .delay(0.0)
                            .show(ui);
                    }
                });

            Panel::hstack()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .transform(palantir::TranslateScale::from_translation(
                    state.scroll_offset,
                ))
                .show(ui, |ui| {
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

                    Panel::vstack()
                        .gap(10.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            // Per-frame `Rc::from([...])` / `Vec::collect()` would each
                            // allocate; the strict-zero `alloc_free` bench catches it.
                            // Cache the canonical `Rc<[Track]>`s once per thread so the
                            // hot path is a refcount bump. Each bench process feeds a
                            // single `scale`, so keying on it isn't needed.
                            thread_local! {
                                static GRID_COLS: OnceCell<Rc<[Track]>> = const { OnceCell::new() };
                                static GRID_ROWS: OnceCell<Rc<[Track]>> = const { OnceCell::new() };
                            }
                            let cols = GRID_COLS.with(|c| {
                                c.get_or_init(|| {
                                    Rc::from([
                                        Track::hug().min(80.0),
                                        Track::fill(),
                                        Track::fixed(60.0),
                                    ])
                                })
                                .clone()
                            });
                            let rows = GRID_ROWS.with(|c| {
                                c.get_or_init(|| {
                                    (0..prop_rows)
                                        .map(|_| Track::hug())
                                        .collect::<Vec<_>>()
                                        .into()
                                })
                                .clone()
                            });
                            Grid::new()
                                .id_salt("props")
                                .cols(cols)
                                .rows(rows)
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
                                            .text_wrap(TextWrap::Wrap)
                                            .grid_cell((r, 1))
                                            .show(ui);
                                        Button::new()
                                            .id_salt(("pact", row))
                                            .label("Edit")
                                            .grid_cell((r, 2))
                                            .show(ui);
                                    }
                                });

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

                            Panel::canvas()
                                .id_salt("shape-gallery")
                                .size((Sizing::FILL, Sizing::Fixed(140.0)))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    add_shape_gallery(ui);
                                });

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
                                                        .text_wrap(TextWrap::Wrap)
                                                        .size((Sizing::FILL, Sizing::Hug))
                                                        .show(ui);
                                                    });
                                            });
                                    }
                                });

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

                            TextEdit::new(&mut state.notes)
                                .id_salt("notes")
                                .size((Sizing::FILL, Sizing::Fixed(60.0)))
                                .show(ui);
                        });
                });

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
                            // Footer "live counter": the partial-damage
                            // arm mutates `state.tick` each iter. Fixed
                            // width pins layout so the changing digits
                            // can't shift siblings — damage collapses to
                            // this single Text node's arranged rect.
                            Text::new(ui.fmt(format_args!("Frame {:08}", state.tick)))
                                .id_salt("footer-status")
                                .style(TextStyle::default().with_font_size(12.0))
                                .size((Sizing::Fixed(120.0), Sizing::Hug))
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

fn add_shape_gallery(ui: &mut Ui) {
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
