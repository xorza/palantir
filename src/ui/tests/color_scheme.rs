use std::rc::Rc;

use glam::UVec2;

use crate::host::harness::Harness;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::preference::Preference;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme::{self, Dark, Light};
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::ui::resources::UiResources;
use crate::ui::tests::support;
use crate::ui::ui_preferences::UiPreferences;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;

const SURFACE: UVec2 = UVec2::new(200, 100);

fn record(ui: &mut Ui) {
    Button::new().id_salt("apply").label("Apply").show(ui);
}

/// `scheme`'s stock theme on another window color, which no stock theme equals.
fn edited(scheme: ColorScheme) -> Theme {
    let mut theme = support::still_theme(scheme);
    theme.window_bg = RgbaF32::new(0.25, 0.5, 0.75, 1.0);
    theme
}

/// With no transition, a pick made during a record keeps that frame's scheme and theme, asks for
/// a frame, and the next frame shows the other scheme's theme, repainted whole.
#[test]
fn a_pick_shows_from_the_next_frame_and_repaints_it_whole() {
    let mut h = Harness::mono(SURFACE);
    support::set_still_themes(&mut h);
    h.frame(record);
    assert_eq!(h.ui().color_scheme(), Dark, "nothing reports a scheme");

    let mut shown = None;
    let report = h.frame(|ui| {
        ui.set_preferences(UiPreferences {
            scheme: Preference::Fixed(Light),
            ..ui.preferences()
        });
        record(ui);
        shown = Some(ui.color_scheme());
    });
    assert_eq!(shown, Some(Dark), "the frame that picked keeps its scheme");
    assert!(report.repaint_requested, "the pick asks for a frame");
    assert!(Rc::ptr_eq(h.ui().theme(), h.ui().scheme_theme(Dark)));

    let report = h.frame(record);
    assert_eq!(h.ui().color_scheme(), Light);
    assert!(Rc::ptr_eq(h.ui().theme(), h.ui().scheme_theme(Light)));
    assert_eq!(
        *h.ui().scheme_theme(Light).as_ref(),
        support::still_theme(Light)
    );
    assert_eq!(
        report.paint(),
        FramePaint::Full,
        "another ground everywhere"
    );
}

/// `System` follows what the host reports, a pick by hand ignores it, and the report stays
/// readable either way.
#[test]
fn the_system_scheme_shows_under_system_only() {
    let mut h = Harness::mono(SURFACE);
    assert_eq!(h.ui().system_color_scheme(), None);

    h.set_system_color_scheme(Some(Light)).frame(record);
    assert_eq!(h.ui().color_scheme(), Light);

    h.set_system_color_scheme(Some(Dark)).frame(record);
    assert_eq!(h.ui().color_scheme(), Dark);

    h.frame(|ui| {
        ui.set_preferences(UiPreferences {
            scheme: Preference::Fixed(Light),
            ..ui.preferences()
        });
    });
    h.frame(record);
    assert_eq!(h.ui().color_scheme(), Light);
    assert_eq!(h.ui().system_color_scheme(), Some(Dark));
}

/// The preference is app-global: a pick in one window shows in another window of the host from
/// its next frame, and a window opened after it starts in it, while each window keeps its own
/// themes.
#[test]
fn a_pick_in_one_window_shows_in_another() {
    let shared = UiResources::isolated_mono();
    let mut first = Harness::from_resources(shared.clone(), SURFACE);
    let mut second = Harness::from_resources(shared.clone(), SURFACE);
    second.frame(record);
    assert_eq!(second.ui().color_scheme(), Dark);

    first.frame(|ui| {
        ui.set_preferences(UiPreferences {
            scheme: Preference::Fixed(Light),
            ..ui.preferences()
        });
    });
    second.frame(record);
    assert_eq!(second.ui().preferences().scheme, Preference::Fixed(Light));
    assert_eq!(second.ui().color_scheme(), Light);
    assert!(!Rc::ptr_eq(
        first.ui().scheme_theme(Light),
        second.ui().scheme_theme(Light)
    ));

    let third = Harness::from_resources(shared, SURFACE);
    assert_eq!(third.ui().color_scheme(), Light, "before its first frame");
}

/// The theme epoch moves only when the shown theme does: a write to the hidden slot and a pick
/// that resolves to the shown scheme leave it, a write to the shown slot moves it at once with no
/// transition.
#[test]
fn the_theme_epoch_moves_only_with_the_shown_theme() {
    let mut h = Harness::mono(SURFACE);
    support::set_still_themes(&mut h);
    h.frame(record);
    let epoch = h.ui().window_theme.epoch();

    let light = Rc::new(edited(Light));
    h.ui_mut().set_theme(Light, Rc::clone(&light));
    assert_eq!(
        h.ui().window_theme.epoch(),
        epoch,
        "the light slot is hidden"
    );
    assert!(Rc::ptr_eq(h.ui().scheme_theme(Light), &light));

    h.frame(|ui| {
        ui.set_preferences(UiPreferences {
            scheme: Preference::Fixed(Dark),
            ..ui.preferences()
        });
    });
    h.frame(record);
    assert_eq!(
        h.ui().window_theme.epoch(),
        epoch,
        "dark resolves to dark again"
    );

    let held = Theme::clone(h.ui().scheme_theme(Dark));
    h.ui_mut().set_theme(Dark, held);
    assert_eq!(
        h.ui().window_theme.epoch(),
        epoch,
        "an equal theme moves nothing"
    );
    h.ui_mut().set_theme(Dark, edited(Dark));
    assert_eq!(h.ui().window_theme.epoch(), epoch.wrapping_add(1));
}

/// A write to the shown slot during a record asks for the frame that records the rest under it;
/// a write to the hidden slot, of the handle the slot holds, or of an equal theme in a new handle,
/// as an app that sets its theme each frame writes, asks for nothing. With no transition, so no
/// blend owes frames of its own.
#[test]
fn a_shown_theme_write_asks_for_a_frame() {
    let mut h = Harness::mono(SURFACE);
    support::set_still_themes(&mut h);
    h.frame(record);
    let shown = Rc::new(edited(Dark));
    let report = h.frame(|ui| {
        record(ui);
        ui.set_theme(Dark, Rc::clone(&shown));
    });
    assert!(report.repaint_requested);
    h.frame(record);
    let epoch = h.ui().window_theme.epoch();
    let report = h.frame(|ui| {
        record(ui);
        ui.set_theme(Dark, Rc::clone(&shown));
        ui.set_theme(Dark, Theme::clone(&shown));
        ui.set_theme(Light, support::still_theme(Light));
    });
    assert!(!report.repaint_requested);
    assert_eq!(h.ui().window_theme.epoch(), epoch);
}
