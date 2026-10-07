//! Shared workload for the frame and allocation benches and the showcase's
//! `frame bench` page: a designed telemetry-console screen exercising every
//! public layout driver, non-animated widget, shape family and `Brush`
//! variant, chrome shadows, grid spans, `disabled` / `hidden` flattening and
//! the popup/tooltip layers.
//!
//! Widget coverage is enforced: `COVERED` and `EXCLUDED` in `tests.rs`, with a
//! reason per exclusion, are checked against `lib.rs`'s public exports.
//!
//! **Nothing animated belongs here.** `Spinner` and any `PaintAnimation` wake
//! the host every frame, so `frame/cached_*` could never settle and
//! `frame/partial_*` would exceed the single footer-counter rect.
//!
//! It lives in `crate::internals` because the frame benches, the allocation
//! gates and the cascade bench all record this tree. Treat its node
//! structure as frozen: adding or removing nodes retargets every recorded
//! series. The showcase hosts it as a viewer page so restyling the tour
//! can't change what the benches measure.

mod chrome;
pub mod dock_fixture;
mod forms;
mod lists;
mod panes;
mod specimen;
mod stat_strip;
mod tokens;

use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use glam::Vec2;

/// Content multiplier the bench arms record at (the showcase page uses a far
/// smaller one).
pub const BENCH_SCALE: usize = 32;

/// Device pixel ratio of the bench surface.
pub const BENCH_DPR: f32 = 2.0;

/// One 1440p display. `BENCH_SCALE` content is far taller, so the CPU arms
/// record, measure and arrange everything while paint and the GPU arms see
/// only the visible part.
pub const BENCH_SURFACE: glam::UVec2 = glam::UVec2::new(2560, 1440); // 1280x720 @ 2x

/// Persistent state for widgets that mutate user data.
///
/// `tick` drives the footer counter and is the only field the partial-damage
/// arm mutates. The footer Text is `Fixed(120.0)`, so the damage rect is that
/// single node's box.
#[derive(Debug)]
pub struct FrameFixture {
    name: String,
    notes: String,
    enabled: bool,
    role: u8,
    pub(crate) tick: u32,
    /// Translate applied to the body panel after arrange, for the
    /// `frame/scrolling_cpu` arm: moves position without changing layout.
    pub(crate) scroll_offset: Vec2,
    /// Backing values for the settings grid, constant across bench iterations so
    /// they never disturb the damage `Skip` / `Partial` invariants.
    volume: f64,
    mix: f64,
    zoom: f64,
    quality: usize,
    dark_mode: bool,
    /// Divider position for the split-pane card.
    split: f32,
    grid_rows: Vec<Track>,
}

impl Default for FrameFixture {
    fn default() -> Self {
        Self {
            name: String::new(),
            notes: String::new(),
            enabled: true,
            role: 1,
            tick: 0,
            scroll_offset: Vec2::ZERO,
            volume: 0.65,
            mix: 0.35,
            zoom: 42.0_f64,
            quality: 2,
            dark_mode: true,
            split: 0.42,
            grid_rows: Vec::new(),
        }
    }
}

impl FrameFixture {
    /// Record the whole fixture page into `ui` at `scale`.
    pub fn render(&mut self, scale: usize, ui: &mut Ui) {
        let sidebar_items = 5 * scale;
        let chat_messages = 2 * scale;
        let film_cells = 2 * scale;
        let prop_rows = 4 + scale;
        let tag_count = 3 * scale;
        let badge_count = scale;
        self.grid_rows.resize(prop_rows, Track::HUG);

        Panel::vstack()
            .gap(10.0)
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background::fill(tokens::APP_BG))
            .show(ui, |ui| {
                chrome::app_bar(ui);

                Panel::hstack()
                    .id_salt("body")
                    .gap(12.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .transform(TranslateScale::from_translation(self.scroll_offset))
                    .show(ui, |ui| {
                        chrome::sidebar(ui, sidebar_items);

                        // A page scroll, not a bare VStack: an overflowing column paints over the
                        // status bar, occluding the footer counter and collapsing `frame/partial_*`
                        // to no damage. Children must be Hug or Fixed, since a scroll passes infinity
                        // on its main axis.
                        Scroll::vertical()
                            .id_salt("page-scroll")
                            .gap(10.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                // Diverse cards first so they fill the showcase viewport; bulky lists trail.
                                stat_strip::show(ui);
                                forms::request_card(self, ui);
                                forms::settings_card(self, ui);
                                specimen::sheet(ui);
                                panes::panes_card(self, ui);
                                lists::filmstrip(ui, film_cells);
                                lists::activity_card(ui, chat_messages);
                                forms::properties_card(self, ui, prop_rows);
                                lists::tags_card(ui, tag_count, badge_count);
                                forms::notes_card(self, ui);
                            });
                    });

                chrome::status_bar(self, ui);
            });
    }
}

#[cfg(test)]
mod tests;
