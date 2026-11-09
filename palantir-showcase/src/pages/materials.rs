//! Materials: palantir's three stock materials over a page that keeps moving under them, and the
//! same material on a popup's and a context menu's chrome.

use crate::scaffold::api;
use crate::support;
use crate::support::{DemoCell, Kit, Knob, Section};
use palantir::Paint;
use palantir::glam::Vec2;
use palantir::widget::{GlyphFont, MotionRole, PaintAnimation, PaintRepeat, Shape};
use palantir::{
    Align, Border, Button, Chrome, Configure, Corners, EffectName, Lens, Material, Menu, MenuItem,
    MenuTheme, Panel, PopupTrigger, Rect, RgbaF32, Sizing, Switch, Text, Ui, WidgetId, fmt,
};
use std::time::Duration;

/// The size of a look's cell.
const CELL: f32 = 220.0;
/// The id of the button that opens the frosted popup.
pub fn popup_button() -> WidgetId {
    WidgetId::from_hash("materials-popup-button")
}

/// The backdrop inside a look's cell.
const LIVE_BOX: Rect = Rect::new(30.0, 50.0, 160.0, 100.0);

pub fn build(ui: &mut Ui, k: Kit) {
    Section::new("Looks")
        .apis(&[
            api!(Material::blur),
            api!(Material::frosted),
            api!(Material::glass),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Each box wears a stock material, a backdrop effect that shows the pixels \
                 painted before it under its fill, which colors them. The page under it turns, so \
                 each box blurs again every frame; a box over a still page costs nothing. With \
                 transparency reduced, each shows over the window's ground instead.",
            );
            support::tiles(ui, |ui| {
                look(
                    ui,
                    k,
                    "blur — σ 8",
                    Material::blur(8.0),
                    RgbaF32::TRANSPARENT,
                );
                look(
                    ui,
                    k,
                    "frosted — σ 16, filled",
                    Material::frosted(16.0),
                    RgbaF32::new(1.0, 1.0, 1.0, 0.18),
                );
                look(
                    ui,
                    k,
                    "glass — a dense lens, σ 1, filled",
                    Material::glass(Lens {
                        bezel: 32.0,
                        thickness: 28.0,
                        refractive_index: 1.6,
                        dispersion: 0.55,
                        ..Lens::WINDOW_GLASS
                    })
                    .with_blur(1.0),
                    RgbaF32::new(1.0, 1.0, 1.0, 0.1),
                );
            });
        });

    Section::new("On chrome")
        .apis(&[api!(Chrome::with_material), api!(PopupTrigger::on)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A popup and a context menu whose chrome wears a frosted material, over the \
                 turning page. Click the button for the popup; right-click the page for the \
                 menu.",
            );
            chrome_demo(ui, k);
        });

    live(ui, k);
}

/// The live backdrop's knobs: the blur, and how much of the fill covers it.
#[derive(Debug)]
struct LiveBackdrop {
    blur: f32,
    fill: f32,
    /// Whether the floor holds the ink; off, the label reads only as well as the page under it.
    hold: bool,
}

impl Default for LiveBackdrop {
    fn default() -> Self {
        Self {
            blur: 16.0,
            fill: 0.3,
            hold: true,
        }
    }
}

/// One frosted backdrop over the turning page, its blur and its fill from the knobs, its text held
/// to 4.5:1 by the floor its ink and fill call for.
fn live(ui: &mut Ui, k: Kit) {
    Section::new("Live material")
        .apis(&[api!(Material::with_min_luminance_for)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Blur and fill one frosted box. The floor its dark ink and light fill call for \
                 lifts whatever shows through, so the label keeps 4.5:1 at any setting; switch it \
                 off to see the label sink into the dark stripes.",
            );
            let id = WidgetId::from_hash("showcase::materials::live");
            ui.with_state::<LiveBackdrop, _>(id, |ui, l| {
                Knob {
                    label: "blur",
                    range: 0.0..=40.0,
                    step: 1.0,
                    unit: " px",
                }
                .show(ui, k, &mut l.blur);
                Knob {
                    label: "fill",
                    range: 0.0..=0.9,
                    step: 0.05,
                    unit: "",
                }
                .show(ui, k, &mut l.fill);
                let ink = RgbaF32::hex(0x14161a);
                let fill = RgbaF32::WHITE.with_alpha(l.fill);
                support::row(ui, |ui| {
                    Switch::new(&mut l.hold).label("Hold the ink").show(ui);
                });
                let frosted = Material::frosted(l.blur);
                let material = if l.hold {
                    frosted.with_min_luminance_for(ink, fill)
                } else {
                    frosted
                };
                let floor = match material.min_luminance {
                    Some(floor) => fmt!(ui, "the backdrop shows at luminance {floor:.3} or more"),
                    None => ui.intern("no floor: a dark stripe behind the label shows as dark"),
                };
                support::readout(ui, k, "floor", floor);
                DemoCell::new("the knobs' backdrop")
                    .size(CELL, CELL)
                    .show(ui, k, |ui| {
                        Panel::canvas()
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                page(ui, k, Vec2::splat(CELL));
                                ui.add_shape(
                                    Shape::rect().at(LIVE_BOX).corners(Corners::all(14.0)),
                                    Paint::from(fill).with_material(material),
                                );
                                let label = ui.intern("readable");
                                ui.add_shape(
                                    Shape::text(label, GlyphFont::new(18.0)).at_origin(Vec2::new(
                                        LIVE_BOX.min.x + 16.0,
                                        LIVE_BOX.min.y + 36.0,
                                    )),
                                    ink,
                                );
                            });
                    });
            });
        });
}

/// A cell of `label` holding the turning page and one backdrop of `material` and `fill` over it.
fn look(ui: &mut Ui, k: Kit, label: &'static str, material: Material, fill: RgbaF32) {
    DemoCell::new(label).size(CELL, CELL).show(ui, k, |ui| {
        Panel::canvas()
            .id_salt(label)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                page(ui, k, Vec2::splat(CELL));
                let corners = if material.effect == EffectName::GLASS {
                    50.0
                } else {
                    14.0
                };
                ui.add_shape(
                    Shape::rect().at(LIVE_BOX).corners(Corners::all(corners)),
                    Paint::from(fill)
                        .with_border(Border::new(RgbaF32::new(1.0, 1.0, 1.0, 0.25), 1.0))
                        .with_material(material),
                );
            });
    });
}

/// Colored bars and a turning cross filling `size`, so a backdrop over it has detail to blur and a
/// reason to blur again each frame.
fn page(ui: &mut Ui, k: Kit, size: Vec2) {
    for (i, color) in (0..12).zip(k.swatch.all().iter().cycle()) {
        let x = i as f32 * size.x / 12.0;
        ui.add_shape(
            Shape::rect().at(Rect::new(x, 0.0, size.x / 24.0, size.y)),
            *color,
        );
    }
    // Essential: the turning page is what each backdrop shows, so it turns under reduced motion
    // too.
    let spin = PaintAnimation::turn(0.0, 1.0)
        .with_period(Duration::from_secs(8))
        .with_repeat(PaintRepeat::Forever)
        .with_motion(MotionRole::Essential);
    let arm = size.min_element() * 0.8;
    let bar = size.min_element() * 0.12;
    for (w, h) in [(arm, bar), (bar, arm)] {
        let rect = Rect::new((size.x - w) / 2.0, (size.y - h) / 2.0, w, h);
        if let Some(shape) = ui.add_shape(Shape::rect().at(rect), RgbaF32::WHITE) {
            shape.animate(spin);
        }
    }
}

/// The frosted chrome the popup and the menu share: a dark fill, rounded, with a hairline.
fn frosted_chrome() -> Chrome {
    Chrome::rounded(RgbaF32::new(0.08, 0.09, 0.11, 0.45), Corners::all(10.0))
        .with_border(Border::new(RgbaF32::new(1.0, 1.0, 1.0, 0.12), 1.0))
        .with_material(Material::frosted(18.0))
}

/// A wide turning page with a button that opens a frosted popup, and a frosted context menu on
/// the page.
fn chrome_demo(ui: &mut Ui, k: Kit) {
    let size = Vec2::new(360.0, 220.0);
    let page = Panel::zstack()
        .id(WidgetId::from_hash("materials-chrome-page"))
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .child_align(Align::CENTER)
        .show(ui, |ui| {
            Panel::canvas()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| page(ui, k, size));
            let trigger = Button::new()
                .id(popup_button())
                .label("open popup")
                .show(ui);
            PopupTrigger::on(&trigger)
                .padding((14.0, 10.0))
                .chrome(frosted_chrome())
                .show(ui, |ui| {
                    Text::new("a frosted popup").style(&k.t.body()).show(ui);
                });
        })
        .response;
    let mut menu: MenuTheme = ui.theme().menu.clone();
    menu.panel = frosted_chrome();
    Menu::context(&page).style(&menu).show(ui, |ui| {
        for label in ["Copy", "Paste", "Delete"] {
            MenuItem::new(label).id_salt(label).show(ui);
        }
    });
}
