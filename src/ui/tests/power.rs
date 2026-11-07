//! Reduced power, as the system's saver mode asks or a preference picks: one value a frame.

use crate::host::harness::Harness;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::ui::Ui;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::tests::support::SURFACE;
use crate::ui::ui_preferences::UiPreferences;

fn pick(ui: &mut Ui, power: Preference<Reduction>) {
    ui.set_preferences(UiPreferences {
        power,
        ..ui.preferences()
    });
}

/// A pick made inside a frame leaves that frame's power as it started, and the next frame, a full
/// record, runs under it. The system's saver mode reaches a frame only through `System`: a pick
/// of full holds over it, and a return to `System` follows it.
#[test]
fn power_moves_only_between_frames_and_follows_the_system_through_system() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |_| {});
    assert!(!h.ui().is_power_reduced());

    let mut seen = None;
    let report = h.frame(|ui| {
        pick(ui, Preference::Fixed(Reduction::Reduced));
        seen = Some(ui.is_power_reduced());
    });
    assert_eq!(
        seen,
        Some(false),
        "the frame keeps the power it started with"
    );
    assert!(report.repaint_requested, "the move asks for a frame");
    let report = h.frame(|_| {});
    assert!(
        h.ui().is_power_reduced(),
        "the next frame runs under the pick"
    );
    assert_ne!(report.processing, FrameProcessing::PaintOnly);

    h.frame(|ui| pick(ui, Preference::Fixed(Reduction::Full)));
    h.set_system_power_reduced(true);
    h.frame(|_| {});
    assert!(h.ui().system_power_reduced());
    assert!(
        !h.ui().is_power_reduced(),
        "a pick of full over the saver mode"
    );

    h.frame(|ui| pick(ui, Preference::System));
    h.frame(|_| {});
    assert!(h.ui().is_power_reduced(), "System follows the saver mode");
}
