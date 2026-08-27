//! The fingerprint that lets a frame reuse the previous cascade, and
//! everything that busts it.

use crate::Ui;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::widgets::frame::Frame;
use glam::{UVec2, Vec2};

/// O5 stage 0: an unchanged frame skips the cascade (its output is
/// provably identical); any cascade-input change — authoring or the
/// exact surface — re-runs it. Pinned via `dbg_cascade_ran`.
#[test]
fn cascade_skip_fires_on_unchanged_reruns_on_change() {
    use crate::layout::types::sizing::Sizing;

    fn build(ui: &mut Ui, w: f32) {
        Frame::new()
            .id(WidgetId::from_hash("f"))
            .size((Sizing::fixed(w), Sizing::fixed(50.0)))
            .show(ui);
    }

    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(ui, 50.0));
    assert!(
        h.ui.frame_runtime.cascade_ran(),
        "first frame runs the cascade"
    );

    h.frame(|ui| build(ui, 50.0));
    assert!(
        !h.ui.frame_runtime.cascade_ran(),
        "unchanged frame skips the cascade"
    );

    h.frame(|ui| build(ui, 80.0));
    assert!(
        h.ui.frame_runtime.cascade_ran(),
        "authoring change re-runs the cascade"
    );

    h.frame(|ui| build(ui, 80.0));
    assert!(
        !h.ui.frame_runtime.cascade_ran(),
        "settles back to skipping"
    );

    h.resize(UVec2::new(SURFACE.x + 1, SURFACE.y));
    h.frame(|ui| build(ui, 80.0));
    assert!(
        h.ui.frame_runtime.cascade_ran(),
        "exact-surface change re-runs the cascade"
    );
}

/// O5 stage-0 completeness for the *authoring* cascade inputs. The
/// fingerprint trusts `subtree_hash` to capture everything the cascade
/// reads (transforms, clip / disabled / focusable, visibility, chrome,
/// shapes); if a future input stops being folded in, a frame toggling
/// it would wrongly skip the cascade and paint stale. One arm per
/// attribute class — each toggles a single attribute and asserts the
/// skip is busted. Scroll offset and zoom are authored transforms and
/// are pinned separately by
/// `widgets::scroll::tests::cascade_skip_busts_on_scroll_offset_change`.
#[test]
fn cascade_fingerprint_covers_authoring_input_classes() {
    use crate::layout::types::clip_mode::ClipMode;
    use crate::scene::visibility::Visibility;

    fn probe(ui: &mut Ui, cfg: impl FnOnce(Frame) -> Frame) {
        cfg(Frame::new().id(WidgetId::from_hash("probe")).size(50.0)).show(ui);
    }

    // Settle `base` into the skip, then run `changed` and assert the
    // one-attribute delta re-runs the cascade.
    fn assert_reruns(label: &str, base: impl Fn(&mut Ui), changed: impl Fn(&mut Ui)) {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| base(ui));
        assert!(
            h.ui.frame_runtime.cascade_ran(),
            "{label}: first frame runs the cascade"
        );
        h.frame(|ui| base(ui));
        assert!(
            !h.ui.frame_runtime.cascade_ran(),
            "{label}: unchanged frame skips the cascade"
        );
        h.frame(|ui| changed(ui));
        assert!(
            h.ui.frame_runtime.cascade_ran(),
            "{label}: toggling it must re-run the cascade — the input is \
             missing from subtree_hash / the cascade fingerprint",
        );
    }

    fn bg(r: f32, g: f32, b: f32) -> Background {
        Background {
            fill: Color::rgb(r, g, b).into(),
            ..Default::default()
        }
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

/// O5 stage-0 completeness for the *identity* cascade inputs: the
/// layer a root subtree lives on and the root's own `WidgetId`.
/// Neither reaches any subtree hash (`compute_rollups` folds only
/// child ids into parents, and roots have no parent), so the
/// fingerprint folds them explicitly. A wrongly matching fingerprint
/// here reuses per-layer cascade columns sized for the previous
/// layer assignment (index OOB in the damage pass) or a `by_id` map
/// still keyed by the dead old root id (inert widget).
#[test]
fn cascade_fingerprint_covers_layer_and_root_identity() {
    fn float(ui: &mut Ui, layer: Layer, key: &str) {
        Frame::new()
            .id(WidgetId::from_hash("anchor"))
            .size(50.0)
            .show(ui);
        ui.layer(layer).at(Vec2::new(10.0, 10.0)).show(|ui| {
            Frame::new()
                .id(WidgetId::from_hash(key))
                .size(20.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8).into(),
                    ..Default::default()
                })
                .show(ui);
        });
    }
    let assert_reruns = |label: &str, base: &dyn Fn(&mut Ui), changed: &dyn Fn(&mut Ui)| {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| base(ui));
        h.frame(|ui| base(ui));
        assert!(
            !h.ui.frame_runtime.cascade_ran(),
            "{label}: unchanged frame skips the cascade"
        );
        h.frame(|ui| changed(ui));
        assert!(
            h.ui.frame_runtime.cascade_ran(),
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
