//! The `CascadeKey` that lets a frame reuse the previous cascade, and what moves it.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::text::font_scope::internals::INTER;
use crate::ui::tests::support::SURFACE;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};

/// An unchanged frame skips the cascade; any input change (authoring, or a surface change moving an arranged rect) re-runs it.
#[test]
fn cascade_skip_fires_on_unchanged_reruns_on_change() {
    use crate::primitives::layout::sizing::Sizing;

    fn build(ui: &mut Ui, w: f32) {
        Block::new()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(w), Sizing::fixed(50.0)))
            .show(ui);
    }

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(ui, 50.0));
    assert!(
        h.engines.cascade.counters.ran(),
        "first frame runs the cascade"
    );

    h.frame(|ui| build(ui, 50.0));
    assert!(
        !h.engines.cascade.counters.ran(),
        "unchanged frame skips the cascade"
    );

    h.frame(|ui| build(ui, 80.0));
    assert!(
        h.engines.cascade.counters.ran(),
        "authoring change re-runs the cascade"
    );

    h.frame(|ui| build(ui, 80.0));
    assert!(
        !h.engines.cascade.counters.ran(),
        "settles back to skipping"
    );

    h.resize(UVec2::new(SURFACE.x + 1, SURFACE.y));
    h.frame(|ui| build(ui, 80.0));
    assert!(
        h.engines.cascade.counters.ran(),
        "a surface change that moves a rect re-runs the cascade"
    );
}

/// Key completeness for authoring inputs: the key trusts tree hashes to capture everything the cascade reads. One arm per attribute class toggles one attribute and asserts the skip is busted. Scroll offset and zoom are pinned by `widgets::scroll::tests::cascade_skip_busts_on_scroll_offset_change`.
#[test]
fn the_key_covers_authoring_input_classes() {
    use crate::primitives::layout::clip_mode::ClipMode;
    use crate::primitives::layout::visibility::Visibility;

    fn probe(ui: &mut Ui, cfg: impl FnOnce(Block) -> Block) {
        cfg(Block::new().id(WidgetId::from_hash("probe")).size(50.0)).show(ui);
    }

    // Settle `base` into the skip, then assert the one-attribute delta re-runs.
    fn assert_reruns(label: &str, base: impl Fn(&mut Ui), changed: impl Fn(&mut Ui)) {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| base(ui));
        assert!(
            h.engines.cascade.counters.ran(),
            "{label}: first frame runs the cascade"
        );
        h.frame(|ui| base(ui));
        assert!(
            !h.engines.cascade.counters.ran(),
            "{label}: unchanged frame skips the cascade"
        );
        h.frame(|ui| changed(ui));
        assert!(
            h.engines.cascade.counters.ran(),
            "{label}: toggling it must re-run the cascade — the input is \
             missing from the tree hashes the cascade key folds",
        );
    }

    fn bg(r: f32, g: f32, b: f32) -> Background {
        Background::fill(RgbaF32::srgb(r, g, b))
    }

    assert_reruns(
        "disabled",
        |ui| probe(ui, |f| f.disabled(false)),
        |ui| probe(ui, |f| f.disabled(true)),
    );
    assert_reruns(
        "focusable",
        |ui| probe(ui, |f| f.focusable(false)),
        |ui| probe(ui, |f| f.focusable(true)),
    );
    assert_reruns(
        "visibility",
        |ui| probe(ui, |f| f.visibility(Visibility::Visible)),
        |ui| probe(ui, |f| f.visibility(Visibility::Hidden)),
    );
    assert_reruns(
        "clip",
        |ui| probe(ui, |f| f.clip(ClipMode::None)),
        |ui| probe(ui, |f| f.clip(ClipMode::Rect)),
    );
    assert_reruns(
        "chrome",
        |ui| probe(ui, |f| f.background(bg(0.2, 0.4, 0.8))),
        |ui| probe(ui, |f| f.background(bg(0.8, 0.2, 0.2))),
    );
}

/// Key completeness for the one input that is not hashed: a font load can change a run's ink in a rect that doesn't move, so the epoch is folded in directly.
#[test]
fn the_key_covers_the_font_database() {
    let mut h = UiHarness::with_text(SURFACE);
    let record = |ui: &mut Ui| {
        Text::new("a label that sizes to its own text")
            .id(WidgetId::from_hash("label"))
            .show(ui);
    };
    h.prime(2, record);
    assert!(
        !h.engines.cascade.counters.ran(),
        "premise: an unchanged tree skips the cascade",
    );

    h.ui.load_font(INTER).expect("the bundled Inter loads");
    h.frame(record);
    assert!(
        h.engines.cascade.counters.ran(),
        "a font load must re-run the cascade — the epoch is missing from \
         the key",
    );
}

/// Key completeness for identity inputs: the root's layer and `WidgetId` reach no subtree hash. A wrongly matching key would reuse per-layer columns sized for the old layer assignment (index OOB) or a `by_id` map keyed by a dead root id.
#[test]
fn the_key_covers_layer_and_root_identity() {
    fn float(ui: &mut Ui, layer: Layer, key: &str) {
        Block::new()
            .id(WidgetId::from_hash("anchor"))
            .size(50.0)
            .show(ui);
        ui.layer(layer).fixed_at(Vec2::new(10.0, 10.0)).show(|ui| {
            Block::new()
                .id(WidgetId::from_hash(key))
                .size(20.0)
                .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                .show(ui);
        });
    }
    let assert_reruns = |label: &str, base: &dyn Fn(&mut Ui), changed: &dyn Fn(&mut Ui)| {
        let mut h = UiHarness::new(SURFACE);
        h.prime(2, |ui| base(ui));
        assert!(
            !h.engines.cascade.counters.ran(),
            "{label}: unchanged frame skips the cascade"
        );
        h.frame(|ui| changed(ui));
        assert!(
            h.engines.cascade.counters.ran(),
            "{label}: identity change must re-run the cascade",
        );
    };
    assert_reruns(
        "layer migration",
        &|ui| float(ui, Layer::Popup, "float"),
        &|ui| float(ui, Layer::Tooltip, "float"),
    );
    assert_reruns(
        "root re-key",
        &|ui| float(ui, Layer::Popup, "float"),
        &|ui| float(ui, Layer::Popup, "float2"),
    );
}
