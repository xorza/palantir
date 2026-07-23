//! Shared workload for the frame and allocation benches and the `frame_visual`
//! example. A synthetic but realistic UI tree exercising every public
//! layout driver (HStack/VStack/ZStack/Canvas/Grid/WrapStack/Scroll),
//! every public widget (Panel/Frame/Button/Text/Grid/Scroll/Checkbox/
//! RadioButton/Switch/Slider/DragValue/ComboBox/ProgressBar/
//! Separator/TextEdit/Tooltip/Popup), every authoring shape family
//! (Rect / Curve / Polyline / Mesh / Text), every `Brush` variant (Solid / Linear / Radial /
//! Conic), and the popup/tooltip layers.

use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use crate::layout::types::align::Align;
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::brush::gradient::conic::ConicGradient;
use crate::primitives::brush::gradient::linear::LinearGradient;
use crate::primitives::brush::gradient::radial::RadialGradient;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::scene::element::Configure;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::drag_value::DragValue;
use crate::widgets::frame::Frame;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::progress_bar::ProgressBar;
use crate::widgets::radio::RadioButton;
use crate::widgets::scroll::Scroll;
use crate::widgets::separator::Separator;
use crate::widgets::slider::Slider;
use crate::widgets::switch::Switch;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::tooltip::Tooltip;

pub(crate) const BENCH_SCALE: usize = 32;

/// Persistent state for widgets that mutate user data (TextEdit needs
/// a `&mut String`, Checkbox a `&mut bool`, RadioButton a `&mut T`).
///
/// `tick` drives the footer-status counter and is the **only** field
/// the partial-damage arm mutates between iterations. The footer Text
/// node is sized `Fixed(120.0)` so the changing digits don't shift
/// sibling layout — the damage rect collapses to that single node's
/// arranged box.
#[derive(Debug)]
pub struct FrameFixture {
    name: String,
    notes: String,
    enabled: bool,
    role: u8,
    pub(crate) tick: u32,
    /// Post-arrange translate applied to the main content panel. Used
    /// by the `frame/scrolling_cpu` bench arm to model continuous
    /// position change WITHOUT changing layout — the cascade walks the
    /// full subtree but layout/measure cache hits trivially. Tests
    /// whether a cascade delta-cache (cached output translated by
    /// `parent_transform`) would meaningfully reduce cascade cost.
    pub(crate) scroll_offset: glam::Vec2,
    /// Backing values for the controls row (Slider / DragValue /
    /// ComboBox / Switch). Held constant across bench iterations —
    /// only `tick` mutates — so they never perturb the steady-state
    /// damage `Skip` / `Partial` invariants the arms assert; they widen
    /// widget coverage only. Seeded to mid-range values so the visual
    /// harness shows them in a representative, non-empty state.
    volume: f32,
    zoom: f64,
    quality: usize,
    dark_mode: bool,
    grid_rows: Vec<Track>,
}

impl Default for FrameFixture {
    fn default() -> Self {
        Self {
            name: String::new(),
            notes: String::new(),
            enabled: true,
            role: 1,
            tick: 0,
            scroll_offset: glam::Vec2::ZERO,
            volume: 0.65,
            zoom: 42.0_f64,
            quality: 2,
            dark_mode: true,
            grid_rows: Vec::new(),
        }
    }
}

impl FrameFixture {
    pub fn render(&mut self, scale: usize, ui: &mut Ui) {
        build_ui(self, scale, ui);
    }
}

pub(crate) fn build_ui(state: &mut FrameFixture, scale: usize, ui: &mut Ui) {
    let sidebar_items = 5 * scale;
    let chat_messages = 2 * scale;
    let canvas_dots = 3 * scale;
    let prop_rows = 4 + scale;
    let tag_count = 3 * scale;
    state.grid_rows.resize(prop_rows, Track::hug());

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
                .size((Sizing::FILL, Sizing::HUG))
                .child_align(Align::CENTER)
                .show(ui, |ui| {
                    Text::new("Aperture — Frame Bench")
                        .id_salt("title")
                        .style(&TextStyle::default().with_font_size(20.0))
                        .show(ui);
                    Frame::new()
                        .id_salt("title-spacer")
                        .size((Sizing::FILL, Sizing::fixed(1.0)))
                        .show(ui);
                    for i in 0..5 {
                        let label = ui.fmt(format_args!("Action {i}"));
                        let btn = Button::new()
                            .id_salt(("hdr", i))
                            .label(label)
                            .show(ui)
                            .snapshot();
                        Tooltip::on(&btn)
                            .text("Header action")
                            .delay(Duration::ZERO)
                            .show(ui);
                    }
                });

            Panel::hstack()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .transform(TranslateScale::from_translation(state.scroll_offset))
                .show(ui, |ui| {
                    Panel::vstack()
                        .gap(4.0)
                        .padding(8.0)
                        .size((Sizing::fixed(220.0), Sizing::FILL))
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
                                            .size((Sizing::FILL, Sizing::HUG))
                                            .show(ui);
                                    }
                                });
                            Frame::new()
                                .id_salt("sb-divider")
                                .size((Sizing::FILL, Sizing::fixed(1.0)))
                                .margin(4.0)
                                .show(ui);
                            Panel::hstack()
                                .gap(2.0)
                                .justify(Justify::Center)
                                .size((Sizing::FILL, Sizing::HUG))
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
                            // Children are ordered diverse-first: the visually varied
                            // widgets lead so they fill the `frame_visual` viewport, while
                            // the bulky repetitive lists (property grid, tag cloud) trail at
                            // the bottom. All stay direct siblings, so the bench's painted
                            // tree (tall offscreen target, everything fits) is identical
                            // regardless of order.
                            Panel::hstack()
                                .id_salt("form-row")
                                .gap(8.0)
                                .padding(6.0)
                                .child_align(Align::CENTER)
                                .size((Sizing::FILL, Sizing::HUG))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    TextEdit::new(&mut state.name)
                                        .id_salt("edit-name")
                                        .size((Sizing::fill(2.0), Sizing::HUG))
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

                            Panel::vstack()
                                .id_salt("controls")
                                .gap(8.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::HUG))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    Panel::hstack()
                                        .id_salt("controls-row")
                                        .gap(10.0)
                                        .child_align(Align::CENTER)
                                        .size((Sizing::FILL, Sizing::HUG))
                                        .show(ui, |ui| {
                                            Switch::new(&mut state.dark_mode)
                                                .id_salt("dark-mode")
                                                .label("dark mode")
                                                .show(ui);
                                            Separator::vertical().id_salt("controls-vsep").show(ui);
                                            let quality_opts = ["Low", "Medium", "High", "Ultra"];
                                            ComboBox::new(&mut state.quality, &quality_opts)
                                                .id_salt("quality")
                                                .size((Sizing::fixed(140.0), Sizing::HUG))
                                                .show(ui);
                                            DragValue::new(&mut state.zoom)
                                                .id_salt("zoom")
                                                .speed(0.5)
                                                .range(0.0..=100.0)
                                                .decimals(0)
                                                .suffix("%")
                                                .size((Sizing::fixed(90.0), Sizing::HUG))
                                                .show(ui);
                                        });
                                    Panel::hstack()
                                        .id_salt("slider-row")
                                        .gap(8.0)
                                        .child_align(Align::CENTER)
                                        .size((Sizing::FILL, Sizing::HUG))
                                        .show(ui, |ui| {
                                            Text::new("Volume")
                                                .id_salt("vol-label")
                                                .style(&TextStyle::default().with_font_size(13.0))
                                                .size((Sizing::fixed(56.0), Sizing::HUG))
                                                .show(ui);
                                            Slider::new(&mut state.volume, 0.0..=1.0)
                                                .id_salt("volume")
                                                .step(0.05)
                                                .show(ui);
                                        });
                                    Separator::horizontal().id_salt("controls-hsep").show(ui);
                                    ProgressBar::new(0.62).id_salt("progress").show(ui);
                                });

                            Panel::canvas()
                                .id_salt("shape-gallery")
                                .size((Sizing::FILL, Sizing::fixed(140.0)))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    add_shape_gallery(ui);
                                });

                            Panel::canvas()
                                .id_salt("dot-canvas")
                                .size((Sizing::FILL, Sizing::fixed(80.0)))
                                .show(ui, |ui| {
                                    for i in 0..canvas_dots {
                                        Frame::new()
                                            .id_salt(("dot", i))
                                            .size((Sizing::fixed(16.0), Sizing::fixed(16.0)))
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
                                            .size((Sizing::FILL, Sizing::HUG))
                                            .show(ui, |ui| {
                                                Frame::new()
                                                    .id_salt(("avatar", i))
                                                    .size((
                                                        Sizing::fixed(40.0),
                                                        Sizing::fixed(40.0),
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
                                                    .size((Sizing::FILL, Sizing::HUG))
                                                    .show(ui, |ui| {
                                                        let name = ui.fmt(format_args!("user_{i}"));
                                                        Text::new(name)
                                                            .id_salt(("from", i))
                                                            .style(
                                                                &TextStyle::default()
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
                                                            &TextStyle::default()
                                                                .with_font_size(13.0),
                                                        )
                                                        .text_wrap(TextWrap::Wrap)
                                                        .size((Sizing::FILL, Sizing::HUG))
                                                        .show(ui);
                                                    });
                                            });
                                    }
                                });

                            Grid::new()
                                .id_salt("props")
                                .cols([Track::hug().min(80.0), Track::fill(), Track::fixed(60.0)])
                                .rows(state.grid_rows.as_slice())
                                .gap(6.0)
                                .padding(4.0)
                                .size((Sizing::FILL, Sizing::HUG))
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
                                            .style(&TextStyle::default().with_font_size(14.0))
                                            .grid_cell((r, 0))
                                            .show(ui);
                                        Text::new(values[row % values.len()])
                                            .id_salt(("pval", row))
                                            .style(&TextStyle::default().with_font_size(14.0))
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

                            Panel::wrap_hstack()
                                .id_salt("tags")
                                .gap(4.0)
                                .padding(6.0)
                                .size((Sizing::FILL, Sizing::HUG))
                                .background(panel_bg.clone())
                                .show(ui, |ui| {
                                    for i in 0..tag_count {
                                        let label = ui.fmt(format_args!("#tag{i}"));
                                        Button::new().id_salt(("tag", i)).label(label).show(ui);
                                    }
                                });

                            TextEdit::new(&mut state.notes)
                                .id_salt("notes")
                                .size((Sizing::FILL, Sizing::fixed(60.0)))
                                .show(ui);
                        });
                });

            Panel::zstack()
                .size((Sizing::FILL, Sizing::fixed(36.0)))
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
                                .style(&TextStyle::default().with_font_size(12.0))
                                .size((Sizing::fixed(120.0), Sizing::HUG))
                                .show(ui);
                            Frame::new()
                                .id_salt("footer-spacer")
                                .size((Sizing::FILL, Sizing::fixed(1.0)))
                                .show(ui);
                            Text::new("v1.2.3 · many nodes")
                                .id_salt("footer-meta")
                                .style(&TextStyle::default().with_font_size(12.0))
                                .show(ui);
                        });

                    Popup::anchored_to(glam::Vec2::new(20.0, 600.0))
                        .background(panel_bg.clone())
                        .show(ui, |ui, _handle| {
                            Text::new("Popup layer")
                                .id_salt("popup-label")
                                .style(&TextStyle::default().with_font_size(11.0))
                                .show(ui);
                        });
                });
        });
}

fn add_shape_gallery(ui: &mut Ui) {
    ui.add_shape(
        Shape::rect(Rect::new(4.0, 6.0, 60.0, 30.0))
            .corners(6.0)
            .fill(Color::rgb(0.85, 0.30, 0.30))
            .stroke(Stroke::solid(Color::rgb(1.0, 1.0, 1.0), 1.0)),
    );
    ui.add_shape(
        Shape::rect(Rect::new(4.0, 42.0, 60.0, 30.0))
            .corners(6.0)
            .fill(
                LinearGradient::builder(FRAC_PI_2)
                    .stop(0.0, ColorU8::hex(0x1a1a2e))
                    .stop(1.0, ColorU8::hex(0x4c5cdb)),
            ),
    );
    ui.add_shape(
        Shape::rect(Rect::new(4.0, 78.0, 60.0, 30.0))
            .corners(6.0)
            .fill(RadialGradient::two_stop_centered(
                ColorU8::hex(0xfacc15),
                ColorU8::hex(0x1a1a2e),
            )),
    );
    ui.add_shape(
        Shape::rect(Rect::new(70.0, 6.0, 60.0, 30.0))
            .corners(6.0)
            .fill(ConicGradient::new(
                glam::Vec2::splat(0.5),
                0.0,
                [
                    Stop::new(0.0, ColorU8::hex(0xff5e44)),
                    Stop::new(0.5, ColorU8::hex(0x46c46c)),
                    Stop::new(1.0, ColorU8::hex(0x4c5cdb)),
                ],
            )),
    );

    ui.add_shape(
        Shape::line(
            glam::Vec2::new(140.0, 12.0),
            glam::Vec2::new(240.0, 36.0),
            3.0,
        )
        .brush(Color::rgb(0.2, 0.9, 1.0))
        .cap(LineCap::Round),
    );
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
    ui.add_shape(
        Shape::polyline(&zigzag, PolylineColors::PerPoint(&zigzag_cols), 4.0).join(LineJoin::Round),
    );

    ui.add_shape(
        Shape::cubic_bezier(
            glam::Vec2::new(250.0, 30.0),
            glam::Vec2::new(280.0, -10.0),
            glam::Vec2::new(340.0, -10.0),
            glam::Vec2::new(370.0, 30.0),
            5.0,
        )
        .brush(Color::rgb(0.4, 1.0, 0.6))
        .cap(LineCap::Round),
    );
    ui.add_shape(
        Shape::quadratic_bezier(
            glam::Vec2::new(250.0, 70.0),
            glam::Vec2::new(310.0, 30.0),
            glam::Vec2::new(370.0, 70.0),
            4.0,
        )
        .brush(Color::rgb(1.0, 0.85, 0.2))
        .cap(LineCap::Square),
    );

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
    ui.add_shape(Shape::mesh(mesh));
}
