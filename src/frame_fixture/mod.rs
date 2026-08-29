//! Shared workload for the frame and allocation benches, and the showcase's
//! `frame bench` page: a synthetic but *designed* app screen — a dark
//! telemetry console — rather than a pile of widgets. It exercises every public
//! layout driver (HStack/VStack/ZStack/Canvas/Grid/WrapHStack/WrapVStack
//! and Scroll on **both** axes), every non-animated public widget, every
//! authoring shape family (Rect / Triangle / Curve / Polyline / Mesh /
//! Shadow / Text), every `Brush` variant (Solid / Linear / Radial / Conic)
//! at both chrome and shape level, chrome drop shadows, grid cell spans,
//! `disabled` / `hidden` cascade flattening, and the popup/tooltip layers.
//!
//! **The widget half of that claim is enforced, not asserted.** `COVERED`
//! and `EXCLUDED` in this module's `tests.rs` are the list, every exclusion
//! carries its reason, and the two together are checked against `lib.rs`'s
//! public exports — so a widget added to the crate fails the suite until
//! someone decides which side it belongs on. The lists are deliberately
//! not restated here as prose: a widget can silently drop out of a
//! sentence, which is exactly what the check exists to prevent.
//!
//! **Nothing animated belongs in here.** `Spinner` — and any `PaintAnim` —
//! wakes the host every frame by design, so `frame/cached_*` could never
//! settle to no damage and `frame/partial_*` would grow past the single
//! footer-counter rect both arms exist to measure. That, and the three other
//! standing exclusions, are recorded with their reasons in `EXCLUDED`.
//!
//! It sits at the crate root rather than beside any one driver because no
//! driver owns it: the frame benches ([`crate::ui::bench`]), the allocation
//! gates in `tests/alloc/gates.rs` and the cascade bench
//! ([`crate::scene::cascade::bench`]) all record this same tree, and its
//! node structure is what makes their numbers comparable release to
//! release. Treat the structure as frozen — retheming is free, but adding
//! or removing nodes retargets every recorded series at once.
//!
//! That freeze is also why the showcase hosts it as a page rather than
//! sharing the showcase's own scaffolding: the page is a *viewer* for this
//! tree, and a restyle of the surrounding tour must not reach in and change
//! what the benches measure.

mod chrome;
mod forms;
mod lists;
mod panes;
mod specimen;
mod stat_strip;
mod tokens;

use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::translate_scale::TranslateScale;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;

/// Content multiplier the bench arms record at. The showcase page uses a
/// far smaller one — this is sized for the bench's tall offscreen target,
/// not for a window.
#[cfg(any(test, feature = "internals"))]
pub const BENCH_SCALE: usize = 32;

/// Device pixel ratio every bench arm renders at.
#[cfg(any(test, feature = "internals"))]
pub const BENCH_DPR: f32 = 2.0;

/// One 1440p display, which is what the reported numbers are meant to
/// stand for. `BENCH_SCALE = 32` content (36-row prop grid, 96-chip
/// wrap, specimen sheet, 64-cell filmstrip, activity scroll, notes) is
/// far taller than this, so everything past the fold is clipped away and
/// culled: the CPU arms still record, measure and arrange the whole
/// tree, while paint and the GPU arms see only the visible part. Raise
/// it with `--size` to measure the whole fixture painting at once.
#[cfg(any(test, feature = "internals"))]
pub const BENCH_SURFACE: glam::UVec2 = glam::UVec2::new(2560, 1440); // 1280x720 @ 2x

/// Persistent state for widgets that mutate user data (TextEdit needs
/// a `&mut String`, Checkbox a `&mut bool`, RadioButton a `&mut T`).
///
/// `tick` drives the footer-status counter and is the **only** field
/// the partial-damage arm mutates between iterations. The footer Text
/// node is sized `Fixed(120.0)` so the changing digits don't shift
/// sibling layout — the damage rect collapses to that single node's
/// arranged box.
#[derive(Debug)]
pub struct FrameFixture {
    name: String,
    notes: String,
    enabled: bool,
    role: u8,
    pub(crate) tick: u32,
    /// Post-arrange translate applied to the main content panel. Used
    /// by the `frame/scrolling_cpu` bench arm to model continuous
    /// position change WITHOUT changing layout — the cascade walks the
    /// full subtree but layout/measure cache hits trivially. Tests
    /// whether a cascade delta-cache (cached output translated by
    /// `parent_transform`) would meaningfully reduce cascade cost.
    pub(crate) scroll_offset: glam::Vec2,
    /// Backing values for the settings grid (Slider / DragValue /
    /// ComboBox / Switch). Held constant across bench iterations —
    /// only `tick` mutates — so they never perturb the steady-state
    /// damage `Skip` / `Partial` invariants the arms assert; they widen
    /// widget coverage only. Seeded to mid-range values so the visual
    /// harness shows them in a representative, non-empty state.
    volume: f32,
    mix: f32,
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
            scroll_offset: glam::Vec2::ZERO,
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
            .background(Background {
                fill: tokens::APP_BG.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                chrome::app_bar(ui);

                Panel::hstack()
                    .id_salt("body")
                    .gap(12.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .transform(TranslateScale::from_translation(self.scroll_offset))
                    .show(ui, |ui| {
                        chrome::sidebar(ui, sidebar_items);

                        // Page scroll, not a bare VStack: the card column is taller
                        // than a normal window, and an overflowing column paints
                        // over the status bar — which occludes the footer counter
                        // and collapses the `frame/partial_*` arms to no damage.
                        // Clipping the overflow here keeps the counter visible at
                        // every viewport size. Every child must therefore be Hug or
                        // Fixed: a scroll passes ∞ on its main axis, so a `Fill`
                        // child would resolve against nothing.
                        Scroll::vertical()
                            .id_salt("page-scroll")
                            .gap(10.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                // Ordered diverse-first: the visually varied cards lead so
                                // they fill the showcase page's viewport, while the bulky
                                // repetitive lists (properties, tags) trail.
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
