//! The suite's golden names, as a type rather than string literals, so a
//! golden with no fixture fails a test instead of lying in the directory.

use std::env;
use std::fs;

use crate::goldens;

/// One golden the suite draws. A fixture names its golden through this
/// type, so a removed fixture leaves its variant unused, which the lint
/// gate rejects, and a variant missing from [`GoldenName::ALL`] leaves its
/// file an orphan, which [`every_golden_file_belongs_to_a_fixture`] rejects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GoldenName {
    AddShapeRoundedRectLinearGradient,
    ArcShapes,
    ButtonHello,
    ColorFieldAndBars,
    ColorPickerPanel,
    ComboBoxClosed,
    CurveCaps,
    DashboardHidpi,
    DockSplitPanes,
    DragValue,
    FocusRing,
    ExpanderOpenAndClosed,
    FrameFilledWithBorder,
    FrameFixture,
    FrameLinearGradient,
    GridMixedTracks,
    GridTwoHugColsLabelNotClipped,
    InterleavedShapesPaintOrder,
    LineDiagonalAa,
    ModalDialog,
    OverflowingGradientAtlas,
    PolylineBevelJoin,
    PolylineGradient,
    PolylineRoundCaps,
    PolylineRoundJoin,
    PolylineTranslucentJoins,
    ProgressBarHalf,
    RadialAndConicGradient,
    RoundedClipPartiallyOffscreen,
    ScrollHorizontalOverflow,
    ScrollNoBarWhenFits,
    ScrollVerticalOverflow,
    ScrollWithUserPadding,
    ScrollXyOverflow,
    ShowcaseGradientsPage,
    SliderThirtyPercent,
    Spinner,
    SurfaceRoundedClipsFullFillChild,
    TabStrip,
    TabbedView,
    TextParagraph,
    TextRowListBatched,
    ToggleSwitchStates,
    Triangle,
    VstackFillWeights,
    WindowedRectMasksCorners,
    ZstackCenteredButton,
}

impl GoldenName {
    /// Every golden, in name order.
    pub(crate) const ALL: [Self; 47] = [
        Self::AddShapeRoundedRectLinearGradient,
        Self::ArcShapes,
        Self::ButtonHello,
        Self::ColorFieldAndBars,
        Self::ColorPickerPanel,
        Self::ComboBoxClosed,
        Self::CurveCaps,
        Self::DashboardHidpi,
        Self::DockSplitPanes,
        Self::DragValue,
        Self::FocusRing,
        Self::ExpanderOpenAndClosed,
        Self::FrameFilledWithBorder,
        Self::FrameFixture,
        Self::FrameLinearGradient,
        Self::GridMixedTracks,
        Self::GridTwoHugColsLabelNotClipped,
        Self::InterleavedShapesPaintOrder,
        Self::LineDiagonalAa,
        Self::ModalDialog,
        Self::OverflowingGradientAtlas,
        Self::PolylineBevelJoin,
        Self::PolylineGradient,
        Self::PolylineRoundCaps,
        Self::PolylineRoundJoin,
        Self::PolylineTranslucentJoins,
        Self::ProgressBarHalf,
        Self::RadialAndConicGradient,
        Self::RoundedClipPartiallyOffscreen,
        Self::ScrollHorizontalOverflow,
        Self::ScrollNoBarWhenFits,
        Self::ScrollVerticalOverflow,
        Self::ScrollWithUserPadding,
        Self::ScrollXyOverflow,
        Self::ShowcaseGradientsPage,
        Self::SliderThirtyPercent,
        Self::Spinner,
        Self::SurfaceRoundedClipsFullFillChild,
        Self::TabStrip,
        Self::TabbedView,
        Self::TextParagraph,
        Self::TextRowListBatched,
        Self::ToggleSwitchStates,
        Self::Triangle,
        Self::VstackFillWeights,
        Self::WindowedRectMasksCorners,
        Self::ZstackCenteredButton,
    ];

    /// The golden's file stem under `golden/`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::AddShapeRoundedRectLinearGradient => "add_shape_rounded_rect_linear_gradient",
            Self::ArcShapes => "arc_shapes",
            Self::ButtonHello => "button_hello",
            Self::ColorFieldAndBars => "color_field_and_bars",
            Self::ColorPickerPanel => "color_picker_panel",
            Self::ComboBoxClosed => "combo_box_closed",
            Self::CurveCaps => "curve_caps",
            Self::DashboardHidpi => "dashboard_hidpi",
            Self::DockSplitPanes => "dock_split_panes",
            Self::DragValue => "drag_value",
            Self::FocusRing => "focus_ring",
            Self::ExpanderOpenAndClosed => "expander_open_and_closed",
            Self::FrameFilledWithBorder => "frame_filled_with_border",
            Self::FrameFixture => "frame_fixture",
            Self::FrameLinearGradient => "frame_linear_gradient",
            Self::GridMixedTracks => "grid_mixed_tracks",
            Self::GridTwoHugColsLabelNotClipped => "grid_two_hug_cols_label_not_clipped",
            Self::InterleavedShapesPaintOrder => "interleaved_shapes_paint_order",
            Self::LineDiagonalAa => "line_diagonal_aa",
            Self::ModalDialog => "modal_dialog",
            Self::OverflowingGradientAtlas => "overflowing_gradient_atlas",
            Self::PolylineBevelJoin => "polyline_bevel_join",
            Self::PolylineGradient => "polyline_gradient",
            Self::PolylineRoundCaps => "polyline_round_caps",
            Self::PolylineRoundJoin => "polyline_round_join",
            Self::PolylineTranslucentJoins => "polyline_translucent_joins",
            Self::ProgressBarHalf => "progress_bar_half",
            Self::RadialAndConicGradient => "radial_and_conic_gradient",
            Self::RoundedClipPartiallyOffscreen => "rounded_clip_partially_offscreen",
            Self::ScrollHorizontalOverflow => "scroll_horizontal_overflow",
            Self::ScrollNoBarWhenFits => "scroll_no_bar_when_fits",
            Self::ScrollVerticalOverflow => "scroll_vertical_overflow",
            Self::ScrollWithUserPadding => "scroll_with_user_padding",
            Self::ScrollXyOverflow => "scroll_xy_overflow",
            Self::ShowcaseGradientsPage => "showcase_gradients_page",
            Self::SliderThirtyPercent => "slider_thirty_percent",
            Self::Spinner => "spinner",
            Self::SurfaceRoundedClipsFullFillChild => "surface_rounded_clips_full_fill_child",
            Self::TabStrip => "tab_strip",
            Self::TabbedView => "tabbed_view",
            Self::TextParagraph => "text_paragraph",
            Self::TextRowListBatched => "text_row_list_batched",
            Self::ToggleSwitchStates => "toggle_switch_states",
            Self::Triangle => "triangle",
            Self::VstackFillWeights => "vstack_fill_weights",
            Self::WindowedRectMasksCorners => "windowed_rect_masks_corners",
            Self::ZstackCenteredButton => "zstack_centered_button",
        }
    }
}

/// No golden file lies in the directory without a fixture that draws it.
/// Under `UPDATE_GOLDEN`, the orphans are deleted, as an update run
/// rewrites the goldens it touches.
#[test]
fn every_golden_file_belongs_to_a_fixture() {
    let orphans = goldens::goldens().orphans(GoldenName::ALL.map(GoldenName::name));
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
