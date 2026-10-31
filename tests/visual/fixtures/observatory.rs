//! The observatory example's five studies, each in both schemes: the stock light and dark themes
//! over the example's own scaffold, as a person first opens each study.

use glam::UVec2;
use palantir::{App, ColorScheme, Palette};

use crate::app::studies::Study;
use crate::app::{MAIN_WINDOW, Shell};
use crate::golden_name::GoldenName;
use crate::visual_rig::VisualRig;

/// Smaller than the example's window, so ten goldens stay small, and wide enough for the rail
/// and a study side by side.
const SIZE: UVec2 = UVec2::new(1280, 800);

/// `study` in `scheme`, after the frames that build its theme and lay it out.
fn assert_study(study: Study, scheme: ColorScheme, golden: GoldenName) {
    let mut shell = Shell::on(study);
    let img = VisualRig::new()
        .system_color_scheme(Some(scheme))
        .clear(Palette::for_scheme(scheme).window_bg)
        .size(SIZE)
        .settled_frame(3, |ui| shell.record(MAIN_WINDOW, ui))
        .image;
    golden.assert_matches(&img);
}

#[test]
fn observatory_studies_match_golden() {
    use ColorScheme::{Dark, Light};
    use GoldenName as G;
    for (study, scheme, golden) in [
        (Study::SkyChart, Dark, G::ObservatorySkyChartDark),
        (Study::SkyChart, Light, G::ObservatorySkyChartLight),
        (Study::Catalog, Dark, G::ObservatoryCatalogDark),
        (Study::Catalog, Light, G::ObservatoryCatalogLight),
        (Study::Session, Dark, G::ObservatorySessionDark),
        (Study::Session, Light, G::ObservatorySessionLight),
        (Study::Workspace, Dark, G::ObservatoryWorkspaceDark),
        (Study::Workspace, Light, G::ObservatoryWorkspaceLight),
        (Study::Appearance, Dark, G::ObservatoryAppearanceDark),
        (Study::Appearance, Light, G::ObservatoryAppearanceLight),
    ] {
        assert_study(study, scheme, golden);
    }
}
