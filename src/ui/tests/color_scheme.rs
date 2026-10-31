use std::rc::Rc;

use glam::UVec2;

use crate::host::harness::Harness;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme::{Dark, Light};
use crate::theme::scheme_preference::SchemePreference;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::ui::resources::UiResources;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;

const SURFACE: UVec2 = UVec2::new(200, 100);

fn record(ui: &mut Ui) {
    Button::new().id_salt("apply").label("Apply").show(ui);
}

/// A pick made during a record keeps that frame's scheme and theme, asks for a frame, and the
/// next frame shows the other scheme's theme, repainted whole.
#[test]
fn a_pick_shows_from_the_next_frame_and_repaints_it_whole() {
    let mut h = Harness::mono(SURFACE);
    h.frame(record);
    assert_eq!(h.ui_ref().color_scheme(), Dark, "nothing reports a scheme");

    let mut shown = None;
    let report = h.frame(|ui| {
        ui.set_scheme_preference(SchemePreference::Light);
        record(ui);
        shown = Some(ui.color_scheme());
    });
    assert_eq!(shown, Some(Dark), "the frame that picked keeps its scheme");
    assert!(report.repaint_requested, "the pick asks for a frame");
    assert!(Rc::ptr_eq(h.ui_ref().theme(), h.ui_ref().theme_for(Dark)));

    let report = h.frame(record);
    assert_eq!(h.ui_ref().color_scheme(), Light);
    assert!(Rc::ptr_eq(h.ui_ref().theme(), h.ui_ref().theme_for(Light)));
    assert_eq!(*h.ui_ref().theme_for(Light).as_ref(), Theme::stock(Light));
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
    assert_eq!(h.ui_ref().system_color_scheme(), None);

    h.set_system_color_scheme(Some(Light)).frame(record);
    assert_eq!(h.ui_ref().color_scheme(), Light);

    h.set_system_color_scheme(Some(Dark)).frame(record);
    assert_eq!(h.ui_ref().color_scheme(), Dark);

    h.frame(|ui| ui.set_scheme_preference(SchemePreference::Light));
    h.frame(record);
    assert_eq!(h.ui_ref().color_scheme(), Light);
    assert_eq!(h.ui_ref().system_color_scheme(), Some(Dark));
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
    assert_eq!(second.ui_ref().color_scheme(), Dark);

    first.frame(|ui| ui.set_scheme_preference(SchemePreference::Light));
    second.frame(record);
    assert_eq!(second.ui_ref().scheme_preference(), SchemePreference::Light);
    assert_eq!(second.ui_ref().color_scheme(), Light);
    assert!(!Rc::ptr_eq(
        first.ui_ref().theme_for(Light),
        second.ui_ref().theme_for(Light)
    ));

    let third = Harness::from_resources(shared, SURFACE);
    assert_eq!(
        third.ui_ref().color_scheme(),
        Light,
        "before its first frame"
    );
}

/// The theme epoch moves only when the shown theme does: a write to the hidden slot and a pick
/// that resolves to the shown scheme leave it, a write to the shown slot moves it.
#[test]
fn the_theme_epoch_moves_only_with_the_shown_theme() {
    let mut h = Harness::mono(SURFACE);
    h.frame(record);
    let epoch = h.ui_ref().theme_epoch;

    let light = Rc::new(Theme::stock(Light));
    h.ui().set_theme_for(Light, Rc::clone(&light));
    assert_eq!(h.ui_ref().theme_epoch, epoch, "the light slot is hidden");
    assert!(Rc::ptr_eq(h.ui_ref().theme_for(Light), &light));

    h.frame(|ui| ui.set_scheme_preference(SchemePreference::Dark));
    h.frame(record);
    assert_eq!(h.ui_ref().theme_epoch, epoch, "dark resolves to dark again");

    h.ui().set_theme_for(Dark, Theme::stock(Dark));
    assert_eq!(h.ui_ref().theme_epoch, epoch.wrapping_add(1));
}
