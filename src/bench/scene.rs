//! The screen the frame-level cases run over, and what they add around it.
//!
//! palantir's benches take the screen as a parameter: the one they run is the showcase's first
//! page, a crate that depends on this one, as a real application does.

use crate::display::system_scale::SystemScale;
use crate::fmt;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};
use std::fmt::Debug;

/// Device pixel ratio of the bench surface.
pub const BENCH_DPR: SystemScale = SystemScale::new(2.0).unwrap();

/// One 1440p display, a 1280 × 720 window at [`BENCH_DPR`].
pub const BENCH_SURFACE: UVec2 = UVec2::new(2560, 1440);

/// Where the counter sits: inside the [`BENCH_SURFACE`] window, near its far corner.
const COUNTER_POS: Vec2 = Vec2::new(1140.0, 696.0);

/// A screen the frame-level cases record. It holds still when no input reaches it, as a cached
/// frame must paint nothing, so nothing on it animates at rest.
pub trait Scene: Default + Debug {
    /// Record the screen into `ui`, which it fills.
    fn record(&mut self, ui: &mut Ui);

    /// Two widgets on screen the `hovering` case moves the pointer between.
    fn hover_targets(&self) -> [WidgetId; 2];

    /// A text field on screen, empty at the start, the `typing` case types into.
    fn field(&self) -> WidgetId;

    /// The bytes [`Self::field`] held at the last [`Self::record`], so a case sees its key land.
    fn field_len(&self) -> usize;
}

/// One frame of `S`, and the two changes a case makes around it, so a screen needs no code for a
/// bench: a counter in the corner, which damages one small rect, and a transform over the whole
/// screen, which moves every paint and no layout.
#[derive(Debug, Default)]
pub(crate) struct SceneFrame<S> {
    pub(crate) scene: S,
    pub(crate) tick: u32,
    pub(crate) scroll: Vec2,
}

impl<S: Scene> SceneFrame<S> {
    pub(crate) fn record(&mut self, ui: &mut Ui) {
        Panel::zstack()
            .id_salt("bench-frame")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("bench-scene")
                    .size((Sizing::FILL, Sizing::FILL))
                    .transform(TranslateScale::from_translation(self.scroll))
                    .show(ui, |ui| self.scene.record(ui));
                Panel::canvas()
                    .id_salt("bench-counter")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        // Fixed width, so a digit that changes moves nothing else.
                        Text::new(fmt!(ui, "Frame {:08}", self.tick))
                            .id_salt("bench-counter-text")
                            .at(COUNTER_POS)
                            .size((Sizing::fixed(120.0), Sizing::HUG))
                            .show(ui);
                    });
            });
    }
}
