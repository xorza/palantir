//! The `CascadeKey` that lets a frame reuse the previous cascade, and
//! everything that moves it.

use crate::Ui;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::text::font_scope::test_support::INTER;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};

/// An unchanged frame skips the cascade (its output is provably
/// identical); any cascade-input change — authoring, or a surface change
/// that moves an arranged rect — re-runs it.
#[test]
fn cascade_skip_fires_on_unchanged_reruns_on_change() {
    use crate::layout::types::sizing::Sizing;

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

/// Key completeness for the *authoring* cascade inputs. The key trusts the tree
/// hashes to capture everything the cascade reads (transforms, clip / disabled
/// / focusable, visibility, chrome, shapes); if a future input stops being
/// folded in, a frame toggling it would wrongly skip the cascade and paint
/// stale. One arm per attribute class — each toggles a single attribute and
/// asserts the skip is busted. Scroll offset and zoom are authored transforms
/// and are pinned separately by
/// `widgets::scroll::tests::cascade_skip_busts_on_scroll_offset_change`.
#[test]
fn the_key_covers_authoring_input_classes() {
    use crate::layout::types::clip_mode::ClipMode;
    use crate::scene::visibility::Visibility;

    fn probe(ui: &mut Ui, cfg: impl FnOnce(Block) -> Block) {
        cfg(Block::new().id(WidgetId::from_hash("probe")).size(50.0)).show(ui);
    }

    // Settle `base` into the skip, then run `changed` and assert the
    // one-attribute delta re-runs the cascade.
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

/// Key completeness for the one cascade input that is not authoring at
/// all. Every other input reaches the key through a hash of what the
/// frame recorded or arranged; a load can change a run's ink in a rect
/// that does not move, while every hash addressing that run stands still.
/// So the epoch is folded in directly.
///
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

/// Key completeness for the *identity* cascade inputs: the layer a root subtree
/// lives on and the root's own `WidgetId`. Neither reaches any subtree hash
/// (`compute_rollups` folds only child ids into parents, and roots have no
/// parent); the key holds one part per layer, and each part folds every node's
/// id. A wrongly matching key here reuses per-layer cascade columns sized for
/// the previous layer assignment (index OOB in the damage pass) or a `by_id`
/// map still keyed by the dead old root id (inert widget).
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
