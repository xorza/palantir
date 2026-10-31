//! The sky chart study: the generated sky as an atlas chart on palantir's own pan and zoom, over a
//! raw-wgpu sky, with a frosted readout, a hover card, a context menu, and a slew to each
//! selection.

pub(crate) mod backdrop;
mod draw;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use palantir::glam::Vec2;
use palantir::{
    Align, Anchor, BackdropFilter, Block, Chrome, ClickOutside, Configure, ContextMenu, Corners,
    GpuView, MenuItem, Panel, Popup, Rect, ResponseSnapshot, RgbaF32, Scroll, Sense, Sizing, Text,
    TextStyle, Ui, WidgetId, fmt,
};

use crate::app::model::Observatory;
use crate::app::model::appearance::Preset;
use crate::app::model::catalog::ObjectId;
use crate::app::model::sky::{self, CHART, Dms, Hms};
use crate::app::studies::Study;
use crate::app::studies::sky_chart::backdrop::{SkyBackdrop, SkyUniform};
use crate::scaffold::api;
use crate::scaffold::header::Header;
use crate::scaffold::tokens::Tokens;

pub(crate) fn header(ui: &mut Ui, t: Tokens) {
    Header {
        title: "Sky chart",
        blurb: "The generated sky as an atlas chart in RA and Dec, on palantir's own pan and \
                zoom, over a sky drawn with raw wgpu. Click a star to select it; the chart slews \
                to every selection, wherever it was made. Right-click a star to plan it.",
        keys: &["wheel", "Ctrl+wheel", "pinch", "click", "right-click"],
        apis: &[
            api!(Scroll::zoomable),
            api!(Scroll::set_zoom),
            api!(type palantir::GpuView),
            api!(type palantir::RadialGradient),
            api!(BackdropFilter::with_max_luminance_for),
            api!(type palantir::Popup),
        ],
        related_label: "showcase",
        related: Study::SkyChart.showcase_pages(),
        source: "examples/observatory/app/studies/sky_chart/mod.rs",
    }
    .show(ui, t);
}

/// What the chart asks of the app beyond the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChartRequest {
    /// Show the selection in the catalog study.
    ShowInCatalog,
}

pub(crate) fn show(ui: &mut Ui, model: &mut Observatory, _t: Tokens) -> Option<ChartRequest> {
    chart(ui, model, "study")
}

/// The slew's length, and the zoom it closes in to.
const SLEW: Duration = Duration::from_millis(450);
const SLEW_ZOOM: f32 = 2.0;
/// How near the pointer a star must be to answer it, in screen pixels.
const PICK_RADIUS: f32 = 10.0;

/// A move of the view from where it was to an object, eased over [`SLEW`].
#[derive(Clone, Copy, Debug)]
struct Slew {
    start: Duration,
    from: Vec2,
    to: Vec2,
    from_zoom: f32,
    to_zoom: f32,
}

/// What a chart keeps between frames.
#[derive(Debug, Default)]
struct ChartState {
    backdrop: Option<Rc<RefCell<SkyBackdrop>>>,
    slew: Option<Slew>,
    /// The selection the chart last slewed to, so each new one slews once.
    slewed_to: Option<ObjectId>,
    /// The star a right-click opened the menu on, held while the menu is open.
    menu_target: Option<ObjectId>,
}

/// The ease-in-out cubic: slow at both ends, `t` in `0..=1`.
pub(crate) fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// The chart under `key`, so the study and the workspace pane keep their own views.
pub(crate) fn chart(
    ui: &mut Ui,
    model: &mut Observatory,
    key: &'static str,
) -> Option<ChartRequest> {
    let scroll_id = WidgetId::from_hash(("observatory::chart::scroll", key));
    let canvas_id = WidgetId::from_hash(("observatory::chart::canvas", key));
    let state_id = WidgetId::from_hash(("observatory::chart::state", key));
    ui.with_state::<ChartState, _>(state_id, |ui, state| {
        let viewport = ui
            .response_for(scroll_id)
            .rect
            .map_or(Vec2::new(1200.0, 700.0), |rect| {
                Vec2::new(rect.size.w, rect.size.h)
            });
        slew(ui, model, state, scroll_id, viewport);
        let zoom = Scroll::zoom(ui, scroll_id);
        let offset = Scroll::offset(ui, scroll_id);
        let view = Rect::new(
            offset.x / zoom,
            offset.y / zoom,
            viewport.x / zoom,
            viewport.y / zoom,
        );
        let night = model.appearance.preset() == Preset::NightVision;
        let canvas = ui.response_for(canvas_id);
        let hovered = canvas
            .pointer_local
            .and_then(|at| nearest(model, at, view, PICK_RADIUS / zoom));
        if !ContextMenu::is_open(ui, canvas_id) {
            state.menu_target = hovered;
        }
        let backdrop = Rc::clone(
            state
                .backdrop
                .get_or_insert_with(|| Rc::new(RefCell::new(SkyBackdrop::default()))),
        );
        let sky = SkyUniform::new(view.min, Vec2::new(view.size.w, view.size.h), CHART, night);
        let repaint = backdrop.borrow().sky != sky;
        backdrop.borrow_mut().sky = sky;
        let mut request = None;
        Panel::zstack()
            .id_salt(("chart", key))
            .size((Sizing::FILL, Sizing::FILL))
            .clip_rounded()
            .show(ui, |ui| {
                GpuView::new(&backdrop)
                    .id_salt("sky")
                    .repaint(repaint)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui);
                Scroll::both()
                    .id(scroll_id)
                    .zoomable()
                    .hide_bars()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        let response = Panel::canvas()
                            .id(canvas_id)
                            .sense(Sense::CLICK)
                            .size((Sizing::fixed(CHART.x), Sizing::fixed(CHART.y)))
                            .show(ui, |ui| {
                                Block::new().size((Sizing::FILL, Sizing::FILL)).show(ui);
                                draw::sky(ui, model, view, zoom, night);
                            })
                            .response;
                        let (clicked, snapshot) = (response.clicked(), response.snapshot());
                        if clicked && let Some(id) = hovered {
                            model.select(id);
                        }
                        request = menu(ui, model, state.menu_target, &snapshot);
                    });
                hud(ui, canvas.pointer_local, hovered, model, zoom);
            });
        if let Some(id) = hovered.filter(|_| !ContextMenu::is_open(ui, canvas_id)) {
            card(ui, model, id);
        }
        request
    })
}

/// Start a slew when the selection moved since the last one, and step the one in flight.
fn slew(
    ui: &mut Ui,
    model: &Observatory,
    state: &mut ChartState,
    scroll_id: WidgetId,
    viewport: Vec2,
) {
    if model.selection != state.slewed_to {
        state.slewed_to = model.selection;
        if let Some(id) = model.selection {
            let object = model.catalog.get(id);
            let from_zoom = Scroll::zoom(ui, scroll_id);
            let to_zoom = from_zoom.max(SLEW_ZOOM);
            let point = sky::chart_point(object.ra, object.dec);
            state.slew = Some(Slew {
                start: ui.now(),
                from: Scroll::offset(ui, scroll_id) / from_zoom,
                to: point - viewport * 0.5 / to_zoom,
                from_zoom,
                to_zoom,
            });
        }
    }
    let Some(slew) = state.slew else {
        return;
    };
    let t = if ui.is_motion_reduced() {
        1.0
    } else {
        (ui.now().saturating_sub(slew.start)).as_secs_f32() / SLEW.as_secs_f32()
    };
    let p = ease(t);
    let zoom = slew.from_zoom + (slew.to_zoom - slew.from_zoom) * p;
    let corner = slew.from + (slew.to - slew.from) * p;
    Scroll::set_zoom(ui, scroll_id, zoom);
    Scroll::set_offset(ui, scroll_id, corner * zoom);
    if t >= 1.0 {
        state.slew = None;
    } else {
        ui.request_repaint();
    }
}

/// The star nearest `at` within `reach` chart pixels, among those in `view`.
fn nearest(model: &Observatory, at: Vec2, view: Rect, reach: f32) -> Option<ObjectId> {
    if !view.contains(at) {
        return None;
    }
    model
        .catalog
        .iter()
        .map(|(id, object)| (id, sky::chart_point(object.ra, object.dec).distance(at)))
        .filter(|&(_, distance)| distance <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// The context menu on the star under the pointer when it opened.
fn menu(
    ui: &mut Ui,
    model: &mut Observatory,
    target: Option<ObjectId>,
    snapshot: &ResponseSnapshot,
) -> Option<ChartRequest> {
    let target = target?;
    let chosen = ContextMenu::on(snapshot).show(ui, |ui, close| {
        let plan = MenuItem::new("Add to plan").show(ui, close).clicked();
        let copy = MenuItem::new("Copy coordinates").show(ui, close).clicked();
        let catalog = MenuItem::new("Show in catalog").show(ui, close).clicked();
        (plan, copy, catalog)
    });
    let (plan, copy, catalog) = chosen.inner?;
    if plan {
        model.add_to_plan(target);
    }
    if copy {
        let object = model.catalog.get(target);
        let text = format!("{}\t{}", Hms(object.ra), Dms(object.dec));
        ui.clipboard().set_text(&text).ok();
    }
    if catalog {
        model.select(target);
        return Some(ChartRequest::ShowInCatalog);
    }
    None
}

/// The night-dark ink and glass the readout wears in every scheme: it sits on the sky, which is
/// content and stays dark.
const HUD_INK: RgbaF32 = RgbaF32::hex(0xe6e9ef);
const HUD_TINT: RgbaF32 = RgbaF32::new(0.02, 0.025, 0.04, 0.55);

/// The frosted readout: the coordinates under the pointer, the zoom, the star under it.
fn hud(
    ui: &mut Ui,
    pointer: Option<Vec2>,
    hovered: Option<ObjectId>,
    model: &Observatory,
    zoom: f32,
) {
    let style = TextStyle::default()
        .with_color(HUD_INK)
        .with_font_size(12.0);
    Panel::vstack()
        .id_salt("hud")
        .align(Align::BOTTOM_LEFT)
        .margin(12.0)
        .padding((12.0, 8.0))
        .gap(2.0)
        .chrome(
            Chrome::rounded(HUD_TINT, Corners::all(8.0)).with_backdrop_filter(
                BackdropFilter::frosted(14.0).with_max_luminance_for(HUD_INK, HUD_TINT),
            ),
        )
        .show(ui, |ui| {
            let line = match pointer {
                Some(at) => {
                    let (ra, dec) = sky::sky_at(at);
                    fmt!(
                        ui,
                        "RA {}   Dec {}   zoom {zoom:.1}\u{d7}",
                        Hms(ra),
                        Dms(dec)
                    )
                }
                None => fmt!(ui, "zoom {zoom:.1}\u{d7}"),
            };
            Text::new(line).style(&style).show(ui);
            if let Some(id) = hovered {
                let object = model.catalog.get(id);
                Text::new(fmt!(
                    ui,
                    "{}  {}  mag {:.2}",
                    object.name,
                    object.kind.label(),
                    object.mag
                ))
                .style(&style)
                .show(ui);
            }
        });
}

/// The hover card beside the pointer.
fn card(ui: &mut Ui, model: &Observatory, id: ObjectId) {
    let Some(pointer) = ui.pointer_pos() else {
        return;
    };
    let object = model.catalog.get(id);
    let name = fmt!(ui, "{}", object.name);
    let facts = fmt!(
        ui,
        "{}  mag {:.2}  {}",
        object.kind.label(),
        object.mag,
        if model.plan.contains(id) {
            "planned"
        } else {
            ""
        }
    );
    Popup::new(Anchor::at_point(pointer + Vec2::new(14.0, 14.0)))
        .id_salt(("card", id.0))
        .click_outside(ClickOutside::PassThrough)
        .show(ui, |ui, _| {
            Panel::vstack().gap(2.0).show(ui, |ui| {
                Text::new(name).bold().show(ui);
                Text::new(facts).show(ui);
            });
        });
}

#[cfg(test)]
mod tests {
    use crate::app::studies::sky_chart::ease;

    /// The easing's ends and middle, and its symmetry: `ease(t) + ease(1 − t) = 1`.
    #[test]
    fn the_slew_eases_in_and_out() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert_eq!(ease(0.5), 0.5);
        assert_eq!(ease(0.25), 0.0625);
        assert!((ease(0.25) + ease(0.75) - 1.0).abs() < 1e-6);
        assert_eq!(ease(-1.0), 0.0);
    }
}
