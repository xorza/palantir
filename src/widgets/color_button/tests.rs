#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use crate::host::harness::Harness;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::theme::Theme;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::popup::PopupTheme;
use crate::widget_core::configure::Configure;
use crate::widgets::color_button::ColorButton;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::color_picker::swatch_row::SwatchRow;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use glam::{UVec2, Vec2};

fn harness() -> Harness {
    Harness::new(UVec2::new(420, 560))
}

/// Nodes the popup layer holds; zero while closed.
fn panel_nodes(h: &Harness) -> usize {
    h.ui().plane_layout(Layer::Popup).rect.len()
}

/// One click on a chip at the surface's corner; press and release take a frame each.
fn click_chip(h: &mut Harness, mut frame: impl FnMut(&mut Harness)) {
    h.press(Vec2::new(10.0, 10.0), PointerButton::Left);
    frame(h);
    h.release(PointerButton::Left);
    frame(h);
}

/// The chip opens on a click and closes on the next, keeping state itself.
#[test]
fn the_chip_toggles_its_panel() {
    let id = WidgetId::from_hash("color-button-toggle");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let frame = |h: &mut Harness, color: &mut RgbaF32| {
        h.frame(|ui| {
            ColorButton::new(ColorPicker::new(color)).id(id).show(ui);
        });
    };
    frame(&mut h, &mut color);
    assert_eq!(panel_nodes(&h), 0, "a chip starts closed");

    click_chip(&mut h, |h| frame(h, &mut color));
    assert!(
        h.rect(PopupTrigger::popup_id(id)).is_some(),
        "the click opened this chip's panel"
    );

    assert!(ColorButton::is_open(h.ui(), id));
    assert!(
        !PopupTrigger::is_open(h.ui(), id),
        "the button's row is its own, not a popup trigger's"
    );

    h.advance_past_double_click();
    click_chip(&mut h, |h| frame(h, &mut color));
    assert_eq!(panel_nodes(&h), 0, "the second click closed it");
    assert!(!ColorButton::is_open(h.ui(), id));

    ColorButton::open(h.ui_mut(), id);
    frame(&mut h, &mut color);
    assert!(
        h.rect(PopupTrigger::popup_id(id)).is_some(),
        "open shows the panel"
    );
    ColorButton::close(h.ui_mut(), id);
    frame(&mut h, &mut color);
    assert_eq!(panel_nodes(&h), 0, "close hides it");
}

/// The popup shows the picker as configured, under `id.with("picker")` unless the picker names an id. The swatch row is off by default; `SwatchRow::History` and `SwatchRow::Fixed` add rows. `texel_size` is one texel per `n` px, rounded up.
#[test]
fn panel_settings_reach_the_picker() {
    use crate::primitives::paint::color::color_model::ColorModel;
    use crate::widgets::color_surface::ColorSurface;

    type Settings = fn(ColorPicker<'_>) -> ColorPicker<'_>;
    const OWN: [RgbaF32; 2] = [RgbaF32::WHITE, RgbaF32::BLACK];
    const OWN_ID: &str = "color-button-own-picker";
    let id = WidgetId::from_hash("color-button-settings");
    let rows: [(&str, Settings, bool, u32, WidgetId); 5] = [
        ("default", |c| c, false, 4, id.with("picker")),
        (
            "history on",
            |c| c.swatches(SwatchRow::History),
            true,
            4,
            id.with("picker"),
        ),
        (
            "own swatches",
            |c| c.swatches(SwatchRow::Fixed(&OWN)),
            true,
            4,
            id.with("picker"),
        ),
        (
            "texel size 1",
            |c| c.texel_size(1),
            false,
            1,
            id.with("picker"),
        ),
        (
            "own id",
            |c| c.id(WidgetId::from_hash(OWN_ID)),
            false,
            4,
            WidgetId::from_hash(OWN_ID),
        ),
    ];
    for (label, settings, swatch_row, texel, picker) in rows {
        let mut h = harness();
        let mut color = RgbaF32::hex(0x4cd3ff);
        let mut frame = |h: &mut Harness| {
            h.frame(|ui| {
                ColorButton::new(settings(ColorPicker::new(&mut color)))
                    .id(id)
                    .show(ui);
            });
        };
        frame(&mut h);
        click_chip(&mut h, &mut frame);
        assert!(
            h.rect(PopupTrigger::popup_id(id)).is_some(),
            "{label}: premise"
        );
        assert_eq!(
            h.node_of(picker.with("swatches")).is_some(),
            swatch_row,
            "{label}: the swatch row",
        );
        let field = h.rect(picker.with("field")).expect("the field").size;
        let built = h
            .ui()
            .state::<ColorSurface<(ColorModel, f32)>>(picker.with("field"))
            .and_then(ColorSurface::built_size)
            .expect("the field built its texture");
        let want = UVec2::new(
            (field.w / texel as f32).ceil() as u32,
            (field.h / texel as f32).ceil() as u32,
        );
        assert_eq!(built, want, "{label}: field {field:?}");
    }
}

/// Opening the panel leaves the color alone; only a gesture inside changes it.
#[test]
fn opening_the_panel_is_not_an_edit() {
    let id = WidgetId::from_hash("color-button-no-edit");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let before = color;
    for round in 0..2 {
        let changes = h
            .frame_passes(|ui| {
                ColorButton::new(ColorPicker::new(&mut color))
                    .id(id)
                    .show(ui)
                    .changed
            })
            .count_where(|&changed| changed);
        assert_eq!(changes, 0, "round {round}");
        h.press(Vec2::new(10.0, 10.0), PointerButton::Left);
        h.release(PointerButton::Left);
    }
    assert_eq!(color, before);
}

/// One open popup's chrome corners, body height and preview chip side.
#[derive(Debug)]
struct Opened {
    corners: Corners,
    height: f32,
    preview: f32,
}

/// Open the chip under `button` with its picker under `picker`, and measure the popup.
fn open_with(button: Option<&ColorPickerTheme>, picker: Option<&ColorPickerTheme>) -> Opened {
    let id = WidgetId::from_hash("color-button-theme");
    let mut h = harness();
    let mut color = RgbaF32::hex(0x4cd3ff);
    let mut frame = |h: &mut Harness| {
        h.frame(|ui| {
            ColorButton::new(ColorPicker::new(&mut color).style(picker))
                .style(button)
                .id(id)
                .show(ui);
        });
    };
    frame(&mut h);
    click_chip(&mut h, &mut frame);

    let body_id = PopupTrigger::popup_id(id);
    let body = h.node_of(body_id).expect("popup body recorded");
    assert_eq!(body.plane.layer, Layer::Popup);
    let chrome = h
        .ui()
        .tree(body.plane.layer)
        .chrome(body.node)
        .expect("the popup body paints chrome");
    let rect = |id: WidgetId| h.rect(id).expect("arranged");
    Opened {
        corners: chrome.corners,
        height: rect(body_id).size.h,
        preview: rect(id.with("picker").with("preview")).size.h,
    }
}

/// The button's style dresses the popup, its chrome and padding, and leaves the picker handed in
/// as the caller styled it; the picker's own style reaches the panel alone. Differential against
/// stock: the button's style adds the gained padding, the picker's the chip growth.
#[test]
fn the_button_styles_the_popup_and_the_picker_keeps_its_own() {
    let stock = Theme::stock(ColorScheme::Dark).color_picker;
    let custom = ColorPickerTheme {
        // Same stroke as stock, so the folded band matches on both sides.
        popup: PopupTheme {
            panel: Chrome {
                corners: Corners::all(9.0),
                ..stock.popup.panel.clone()
            },
            padding: Spacing::all(19.0),
        },
        chip_size: stock.chip_size + 30.0,
        ..stock.clone()
    };
    assert_ne!(custom.popup.panel.corners, stock.popup.panel.corners);

    let plain = open_with(None, None);
    let button = open_with(Some(&custom), None);
    let picker = open_with(None, Some(&custom));

    assert_eq!(plain.corners, stock.popup.panel.corners, "stock chrome");
    assert_eq!(
        button.corners, custom.popup.panel.corners,
        "the button's chrome"
    );
    assert_eq!(
        picker.corners, stock.popup.panel.corners,
        "the picker's own"
    );
    assert_eq!(plain.preview, stock.chip_size, "stock preview");
    assert_eq!(button.preview, stock.chip_size, "the picker kept its own");
    assert_eq!(picker.preview, custom.chip_size, "the picker's style");

    // Two edges of padding (19 - 8) * 2 = 22; the bars row grows by the preview's 30.
    let padding = custom.popup.padding.sums().h - stock.popup.padding.sums().h;
    assert_eq!(button.height - plain.height, padding);
    assert_eq!(picker.height - plain.height, 30.0);
}

/// The constructor mints its id at the caller's line, so two chips keep two open states; one minted inside the constructor would make every chip a twin of the others.
#[test]
fn each_call_site_mints_its_own_id() {
    fn id_of(mut chip: ColorButton<'_>) -> WidgetId {
        chip.configure().widget.ident.recipe().raw_id(None)
    }
    let mut color = RgbaF32::WHITE;
    let first = id_of(ColorButton::new(ColorPicker::new(&mut color)));
    let second = id_of(ColorButton::new(ColorPicker::new(&mut color)));
    assert_ne!(first, second);
}
