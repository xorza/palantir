//! Shared workload for the frame and allocation benches and the `frame_visual`
//! example: a synthetic but *designed* app screen — a dark telemetry
//! console — rather than a pile of widgets. It exercises every public
//! layout driver (HStack/VStack/ZStack/Canvas/Grid/WrapHStack/WrapVStack
//! and Scroll on **both** axes), every non-animated public widget
//! (Panel/Frame/Button/Text/Grid/Scroll/Checkbox/RadioButton/Switch/
//! Slider/DragValue/ComboBox/ProgressBar/Separator/TextEdit/Tooltip/Popup),
//! every authoring shape family (Rect / Triangle / Curve / Polyline / Mesh /
//! Shadow / Text), every `Brush` variant (Solid / Linear / Radial / Conic)
//! at both chrome and shape level, chrome drop shadows, grid cell spans,
//! `disabled` / `hidden` cascade flattening, and the popup/tooltip layers.
//!
//! **Nothing animated belongs in here.** `Spinner` — and any `PaintAnim` —
//! wakes the host every frame by design, so `frame/cached_*` could never
//! settle to `Damage::Skip` and `frame/partial_*` would grow past the single
//! footer-counter rect both arms exist to measure. `Modal`, `ContextMenu`,
//! and `GpuView` are absent for the same class of reason: the first two
//! record nothing until an interaction the benches never deliver, and
//! `GpuView` needs a `wgpu::Device` the deviceless CPU/alloc harnesses
//! don't have.
//!
//! It sits at the crate root rather than beside any one driver because no
//! driver owns it: the frame benches ([`crate::ui::bench`]), the allocation
//! gates ([`crate::host::bench`]) and the cascade bench
//! ([`crate::scene::cascade::bench`]) all record this same tree, and its
//! node structure is what makes their numbers comparable release to
//! release. Treat the structure as frozen — retheming is free, but adding
//! or removing nodes retargets every recorded series at once.

mod chrome;
mod forms;
mod lists;
mod specimen;
mod stat_strip;
mod tokens;

use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::transform::TranslateScale;
use crate::scene::node::Configure;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;

pub(crate) const BENCH_SCALE: usize = 32;

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
            grid_rows: Vec::new(),
        }
    }
}

impl FrameFixture {
    pub fn render(&mut self, scale: usize, ui: &mut Ui) {
        build_ui(self, scale, ui);
    }
}

pub(crate) fn build_ui(state: &mut FrameFixture, scale: usize, ui: &mut Ui) {
    let sidebar_items = 5 * scale;
    let chat_messages = 2 * scale;
    let film_cells = 2 * scale;
    let prop_rows = 4 + scale;
    let tag_count = 3 * scale;
    let badge_count = scale;
    state.grid_rows.resize(prop_rows, Track::hug());

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
                .transform(TranslateScale::from_translation(state.scroll_offset))
                .show(ui, |ui| {
                    chrome::sidebar(ui, sidebar_items);

                    // Page scroll, not a bare VStack: the card column is taller
                    // than a normal window, and an overflowing column paints
                    // over the status bar — which occludes the footer counter
                    // and collapses the `frame/partial_*` arms to `Damage::Skip`.
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
                            // they fill the `frame_visual` viewport, while the bulky
                            // repetitive lists (properties, tags) trail.
                            stat_strip::show(ui);
                            forms::request_card(state, ui);
                            forms::settings_card(state, ui);
                            specimen::sheet(ui);
                            lists::filmstrip(ui, film_cells);
                            lists::activity_card(ui, chat_messages);
                            forms::properties_card(state, ui, prop_rows);
                            lists::tags_card(ui, tag_count, badge_count);
                            forms::notes_card(state, ui);
                        });
                });

            chrome::status_bar(state, ui);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::frame_report::FramePaint;
    use crate::ui::harness::UiHarness;
    use std::time::Duration;

    /// The `frame/partial_*` arms model an interactive steady state: one
    /// counter changes, everything else holds, so damage collapses to the
    /// footer Text's arranged rect. Pinned here — not only inside
    /// `ui::bench::assert_partial_invariant` — so a fixture edit that lets
    /// the counter reflow its siblings (→ `Full`) or hides the change from
    /// the tree entirely (→ `Skip`) fails `cargo test` instead of quietly
    /// retargeting the bench.
    ///
    /// Swept across viewport sizes because the `Skip` failure mode is a
    /// *layout* bug, not a damage bug: before the card column got its page
    /// scroll it overflowed on a normal-sized window and painted over the
    /// status bar, and an occluded counter damages nothing. The smallest
    /// size here is the one that regressed; the largest is the bench's own
    /// `CACHED_SIZE` / `BENCH_SCALE` pair.
    #[test]
    fn footer_counter_alone_yields_partial_damage() {
        for (px, scale) in [
            (glam::UVec2::new(1280, 800), 6usize),
            (glam::UVec2::new(2560, 1600), 6),
            (glam::UVec2::new(3840, 4800), 32),
        ] {
            let mut h = UiHarness::with_text(px).scale(2.0);
            let mut state = FrameFixture::default();
            let mut paint = FramePaint::Full;
            // Two frames settle the caches and the popup's anchor (it reads
            // last frame's status-bar rect); the rest are steady state.
            for i in 0..5u64 {
                state.tick = state.tick.wrapping_add(1);
                paint = h
                    .at(Duration::from_millis(i * 16))
                    .frame(|ui| build_ui(&mut state, scale, ui))
                    .paint();
            }
            assert_eq!(
                paint,
                FramePaint::Partial,
                "tick-only change must damage just the footer counter at {px:?} @2x, scale {scale}",
            );
        }
    }
}
