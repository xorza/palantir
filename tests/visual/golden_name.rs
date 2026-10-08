//! The suite's golden names as a type, so a golden with no fixture fails a test.

use std::env;
use std::fs;

use crate::goldens;

/// Declares [`GoldenName`] from one `Variant => "file_stem"` list, so the variants, [`GoldenName::ALL`] and [`GoldenName::name`] cannot drift apart.
macro_rules! golden_names {
    ($($variant:ident => $file:literal,)*) => {
        /// One golden the suite draws. A golden file no variant names is an orphan ([`every_golden_file_belongs_to_a_fixture`]); a variant whose fixture is gone still claims its file, so remove it with the fixture.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum GoldenName {
            $($variant,)*
        }

        impl GoldenName {
            /// Every golden.
            pub(crate) const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The golden's file stem under `golden/`.
            pub(crate) const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $file,)*
                }
            }
        }
    };
}

golden_names! {
    AddShapeRoundedRectLinearGradient => "add_shape_rounded_rect_linear_gradient",
    ArcShapes => "arc_shapes",
    ButtonHello => "button_hello",
    ColorFieldAndBars => "color_field_and_bars",
    ColorPickerPanel => "color_picker_panel",
    ComboBoxClosed => "combo_box_closed",
    CurveCaps => "curve_caps",
    DashboardHidpi => "dashboard_hidpi",
    DockSplitPanes => "dock_split_panes",
    DragValue => "drag_value",
    ExpanderOpenAndClosed => "expander_open_and_closed",
    FocusRing => "focus_ring",
    FrameFilledWithBorder => "frame_filled_with_border",
    FrameFixture => "frame_fixture",
    GridMixedTracks => "grid_mixed_tracks",
    GridTwoHugColsLabelNotClipped => "grid_two_hug_cols_label_not_clipped",
    InterleavedShapesPaintOrder => "interleaved_shapes_paint_order",
    LineDiagonalAa => "line_diagonal_aa",
    ModalDialog => "modal_dialog",
    PolylineBevelJoin => "polyline_bevel_join",
    PolylineGradient => "polyline_gradient",
    PolylineRoundCaps => "polyline_round_caps",
    PolylineRoundJoin => "polyline_round_join",
    PolylineTranslucentJoins => "polyline_translucent_joins",
    ProgressBarHalf => "progress_bar_half",
    RoundedClipPartiallyOffscreen => "rounded_clip_partially_offscreen",
    ScrollHorizontalOverflow => "scroll_horizontal_overflow",
    ScrollNoBarWhenFits => "scroll_no_bar_when_fits",
    ScrollVerticalOverflow => "scroll_vertical_overflow",
    ScrollWithUserPadding => "scroll_with_user_padding",
    ScrollXyOverflow => "scroll_xy_overflow",
    ShowcaseGradientsPage => "showcase_gradients_page",
    SliderThirtyPercent => "slider_thirty_percent",
    Spinner => "spinner",
    SurfaceRoundedClipsFullFillChild => "surface_rounded_clips_full_fill_child",
    TabStrip => "tab_strip",
    TabbedView => "tabbed_view",
    TextParagraph => "text_paragraph",
    TextRowListBatched => "text_row_list_batched",
    ToggleSwitchStates => "toggle_switch_states",
    Triangle => "triangle",
    VstackFillWeights => "vstack_fill_weights",
    WindowedRectMasksCorners => "windowed_rect_masks_corners",
    ZstackCenteredButton => "zstack_centered_button",
}

/// No golden file lies in the directory without a fixture that draws it. Under `UPDATE_GOLDEN`, orphans are deleted.
#[test]
fn every_golden_file_belongs_to_a_fixture() {
    let orphans = goldens::goldens().orphans(GoldenName::ALL.iter().map(|golden| golden.name()));
    if env::var_os("UPDATE_GOLDEN").is_some_and(|value| !value.is_empty()) {
        for orphan in &orphans {
            fs::remove_file(orphan).expect("delete an orphaned golden");
        }
        return;
    }
    assert!(
        orphans.is_empty(),
        "goldens no fixture draws: {orphans:?}\nRe-run with UPDATE_GOLDEN=1 to delete them.",
    );
}
