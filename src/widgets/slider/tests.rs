use crate::host::harness::Harness;
use crate::internals::panic_probe;
use crate::internals::size_trio::SizeTrio;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::slider::{Slider, SliderRange};
use glam::{UVec2, Vec2};

const fn range(min: f64, max: f64) -> SliderRange {
    SliderRange {
        min,
        max,
        step: None,
    }
}

/// Explicit `.size(...)` wins over the `Fill × knob_size` default; hugged, it is the knob alone.
#[test]
fn explicit_size_overrides_fill_default() {
    let mut v = 0.5_f64;
    let trio = SizeTrio::of((Sizing::fixed(120.0), Sizing::fixed(30.0)), |ui, size| {
        let mut slider = Slider::new(&mut v, 0.0..=1.0);
        if let Some(size) = size {
            slider = slider.size(size);
        }
        slider.show(ui).response.node(ui)
    });
    assert_eq!(
        trio,
        SizeTrio {
            sized: Size::new(120.0, 30.0),
            hug: Size::new(18.0, 18.0),
            default: Size::new(400.0, 18.0),
        }
    );

    // The knob centers on the cross axis by default, and a caller's `child_align` wins:
    // a 40-tall slider centers the 18 px knob at `(40 - 18) / 2 = 11`.
    let id = WidgetId::from_hash("slider-align");
    let mut h = Harness::mono(UVec2::new(200, 100));
    for (align, y) in [(None, 11.0), (Some(Align::v(VAlign::Top)), 0.0)] {
        h.frame(|ui| {
            let mut slider = Slider::new(&mut v, 0.0..=1.0).id(id).size((200.0, 40.0));
            if let Some(align) = align {
                slider = slider.child_align(align);
            }
            slider.show(ui);
        });
        let knob = h.layout_rect(id.with("knob"));
        assert_eq!(
            knob.unwrap().min.y - h.layout_rect(id).unwrap().min.y,
            y,
            "{align:?}"
        );
    }
}

/// An endpoint collapses one track segment to zero extent, and a NaN value lays out as the low end.
#[test]
fn endpoint_rails_collapse_without_invalid_fill_weights() {
    for (value, expected) in [
        (0.0, [0.0, 18.0, 102.0]),
        (1.0, [102.0, 18.0, 0.0]),
        (f64::NAN, [0.0, 18.0, 102.0]),
    ] {
        let mut h = Harness::mono(UVec2::new(120, 30));
        let mut value = value;
        let root = h.frame_value(|ui| {
            Slider::new(&mut value, 0.0..=1.0)
                .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                .show(ui)
                .response
                .node(ui)
        });
        let widths: Vec<_> = h
            .main_child_rects(root)
            .into_iter()
            .map(|rect| rect.size.w)
            .collect();
        assert_eq!(widths, expected, "value {value}");
    }
}

#[test]
fn fraction_of_maps_and_clamps() {
    let cases = [
        (50.0, 0.0, 100.0, 0.5),
        (0.0, 0.0, 100.0, 0.0),
        (100.0, 0.0, 100.0, 1.0),
        (150.0, 0.0, 100.0, 1.0), // above clamps
        (-10.0, 0.0, 100.0, 0.0), // below clamps
        (15.0, 10.0, 20.0, 0.5),  // offset range
        (5.0, 3.0, 3.0, 0.0),     // degenerate
        (5e-6, 0.0, 1e-5, 0.5),
        (5e99, 0.0, 1e100, 0.5),
        (2.5e-7, 0.0, 1e-5, 0.025),
        (1e300, 0.0, 1.0, 1.0),
        (-1e300, 0.0, 1.0, 0.0),
        (1.0, 0.0, 1e-300, 1.0),
        (50.0, 100.0, 0.0, 0.5),
        (100.0, 100.0, 0.0, 0.0),
        (0.0, 100.0, 0.0, 1.0),
    ];
    for (v, min, max, want) in cases {
        let got = range(min, max).fraction_of(v);
        assert_eq!(got, want, "v2f({v},{min},{max})={got} want {want}");
    }
    for (v, min, max) in [
        (f64::NAN, 0.0, 100.0),
        (50.0, f64::NAN, 100.0),
        (50.0, 0.0, f64::NAN),
    ] {
        assert_eq!(
            range(min, max).fraction_of(v),
            0.0,
            "v2f({v},{min},{max}) must read as the low end",
        );
    }
}

#[test]
fn value_at_inverts_fraction_of() {
    for &v in &[10.0_f64, 12.5, 15.0, 17.5, 20.0] {
        let f = range(10.0, 20.0).fraction_of(v);
        let back = range(10.0, 20.0).value_at(f);
        assert_eq!(back, v, "roundtrip {v} -> {f} -> {back}");
    }
    assert_eq!(range(10.0, 20.0).value_at(0.25), 12.5);
    let f = range(100.0, 0.0).fraction_of(75.0);
    assert_eq!(f, 0.25, "reversed fraction {f}");
    assert_eq!(range(100.0, 0.0).value_at(f), 75.0);
    assert_eq!(range(0.0, 100.0).value_at(1.5), 100.0);
}

#[test]
fn pointer_mapping_is_scale_invariant() {
    let id = WidgetId::from_hash("scaled-slider");
    for scale in [0.5, 1.0, 2.0] {
        for (local_x, expected) in [(9.0, 0.0), (34.5, 0.25), (111.0, 1.0)] {
            let mut h = Harness::mono(UVec2::new(300, 100));
            let mut value = 0.5_f64;
            let build = |ui: &mut Ui, value: &mut f64| {
                Panel::zstack()
                    .id(WidgetId::from_hash("scaled-slider-parent"))
                    .transform(TranslateScale::from_scale(scale))
                    .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                    .show(ui, |ui| {
                        Slider::new(value, 0.0..=1.0)
                            .id(id)
                            .size((Sizing::fixed(120.0), Sizing::fixed(18.0)))
                            .show(ui);
                    });
            };
            h.frame(|ui| build(ui, &mut value));

            h.press(h.point_in(id, Vec2::new(local_x, 9.0)), PointerButton::Left);
            h.frame(|ui| build(ui, &mut value));

            assert_eq!(
                value, expected,
                "logical x={local_x} at {scale}× produced {value}, expected {expected}"
            );
        }
    }
}

/// `None` is the only "off"; a step that cannot snap is refused.
#[test]
fn step_rejects_a_value_that_cannot_snap() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        panic_probe::assert_panics_with("a positive value must be finite and above zero", || {
            let mut v = 0.5_f64;
            let _ = Slider::new(&mut v, 0.0..=1.0).step(bad);
        });
    }
}

/// An infinite or NaN end is refused at build: a click would store `+inf` or `NaN`.
#[test]
fn new_rejects_an_infinite_range() {
    for (lo, hi) in [
        (0.0, f64::INFINITY),
        (f64::NEG_INFINITY, 1.0),
        (f64::NAN, 1.0),
    ] {
        panic_probe::assert_panics_with("a range must have finite ends", || {
            let mut v = 0.5_f64;
            let _ = Slider::new(&mut v, lo..=hi);
        });
    }
}

/// A `BoundNum` integer target lands on whole values: on `0..=10`, x = 34 is 2.5, rounding away from zero to 3.
#[test]
fn an_integer_target_lands_on_whole_values() {
    let id = WidgetId::from_hash("slider-int");
    let mut h = Harness::mono(UVec2::new(118, 18));
    let mut value = 0_i64;
    let frame = |h: &mut Harness, value: &mut i64| {
        h.frame(|ui| {
            Slider::new(&mut *value, 0.0..=10.0)
                .size((Sizing::fixed(118.0), Sizing::fixed(18.0)))
                .id(id)
                .show(ui);
        });
    };
    frame(&mut h, &mut value);

    h.press(Vec2::new(89.0, 9.0), PointerButton::Left);
    frame(&mut h, &mut value);
    assert_eq!(value, 8);

    h.drag_to(Vec2::new(34.0, 9.0));
    frame(&mut h, &mut value);
    assert_eq!(value, 3);
}

/// A focused slider walks by key: arrows a hundredth of the range, Shift ten
/// of those, page keys a tenth, Home/End to the ends. A step snaps, End to the last grid
/// value inside the range; every key
/// is a whole edit; unfocused or disabled, keys move nothing.
#[test]
fn a_focused_slider_walks_by_key() {
    use crate::primitives::input::keyboard::key::Key;
    use crate::primitives::input::keyboard::modifiers::Modifiers;

    /// Modifiers, key, range, step, and where 5.0 lands.
    type Case = (Modifiers, Key, (f64, f64), Option<f64>, f64);
    let id = WidgetId::from_hash("slider-keys");
    let cases: [Case; 12] = [
        (Modifiers::NONE, Key::End, (0.0, 10.0), Some(4.0), 8.0),
        (Modifiers::NONE, Key::ArrowRight, (0.0, 10.0), None, 5.1),
        (Modifiers::NONE, Key::ArrowUp, (0.0, 10.0), None, 5.1),
        (Modifiers::NONE, Key::ArrowLeft, (0.0, 10.0), None, 4.9),
        (Modifiers::SHIFT, Key::ArrowRight, (0.0, 10.0), None, 6.0),
        (Modifiers::NONE, Key::PageUp, (0.0, 10.0), None, 6.0),
        (Modifiers::NONE, Key::PageDown, (0.0, 10.0), None, 4.0),
        (Modifiers::NONE, Key::Home, (0.0, 10.0), None, 0.0),
        (Modifiers::NONE, Key::End, (0.0, 10.0), None, 10.0),
        (
            Modifiers::NONE,
            Key::ArrowRight,
            (0.0, 10.0),
            Some(0.5),
            5.5,
        ),
        (Modifiers::NONE, Key::ArrowRight, (10.0, 0.0), None, 4.9),
        (Modifiers::NONE, Key::End, (10.0, 0.0), None, 0.0),
    ];
    for (mods, key, (min, max), step, want) in cases {
        let mut h = Harness::mono(UVec2::new(200, 40));
        let mut value = 5.0_f64;
        let frame = |h: &mut Harness, value: &mut f64| {
            h.frame_value(|ui| {
                let slider = Slider::new(&mut *value, min..=max).id(id);
                match step {
                    Some(s) => slider.step(s),
                    None => slider,
                }
                .show(ui)
            })
        };
        frame(&mut h, &mut value);
        h.ui_mut().set_focus(id);
        h.hold_modifiers(mods);
        h.key(key);
        let edges = frame(&mut h, &mut value);
        assert_eq!(value, want, "{mods:?} {key:?} over {min}..={max}");
        assert!(edges.changed && edges.committed, "{key:?} is a whole edit");
    }

    let mut h = Harness::mono(UVec2::new(200, 40));
    let mut value = 5.0_f64;
    for (focused, disabled) in [(false, false), (true, true)] {
        if focused {
            h.ui_mut().set_focus(id);
        }
        h.key(Key::ArrowRight);
        h.frame(|ui| {
            Slider::new(&mut value, 0.0..=10.0)
                .id(id)
                .disabled(disabled)
                .show(ui);
        });
        assert_eq!(value, 5.0, "focused {focused}, disabled {disabled}");
    }
}

/// An arrow steps at least one unit of the bound number's last digit: an integer over 0..=10
/// moves 5 → 6, where a hundredth of the range (0.1) rounded back to 5 and the key did nothing;
/// a float at `decimals(0)` does the same. Without a snap step the step adds to the stored
/// integer exactly: past 2^53, `2^53 + 1` and a step of `1e18 / 100 = 1e16` land on
/// `2^53 + 1 + 1e16`, where a round trip through `f64` dropped the 1.
#[test]
fn an_arrow_steps_at_least_one_unit_of_the_bound_number() {
    use crate::primitives::input::keyboard::key::Key;

    let id = WidgetId::from_hash("slider-unit");
    let press = |record: &mut dyn FnMut(&mut Ui)| {
        let mut h = Harness::mono(UVec2::new(200, 40));
        h.frame(&mut *record);
        h.ui_mut().set_focus(id);
        h.key(Key::ArrowRight);
        h.frame(&mut *record);
    };

    let mut whole = 5_i64;
    press(&mut |ui| {
        Slider::new(&mut whole, 0.0..=10.0).id(id).show(ui);
    });
    assert_eq!(whole, 6);

    let mut float = 5.0_f64;
    press(&mut |ui| {
        Slider::new(&mut float, 0.0..=10.0)
            .id(id)
            .decimals(0)
            .show(ui);
    });
    assert_eq!(float, 6.0);

    let mut large = (1_i64 << 53) + 1;
    press(&mut |ui| {
        Slider::new(&mut large, 0.0..=1e18).id(id).show(ui);
    });
    assert_eq!(large, (1_i64 << 53) + 1 + 10_000_000_000_000_000);

    // Decimals past what an `f64` holds are coerced to 15, so the unit over a span of `1e-14`
    // is `1e-15`, not the hundredth `1e-16`.
    let mut fine = 0.0_f64;
    press(&mut |ui| {
        Slider::new(&mut fine, 0.0..=1e-14)
            .id(id)
            .decimals(usize::MAX)
            .show(ui);
    });
    assert_eq!(fine, 1e-15);

    // A half step on an integer steps a whole one each way: 5 + 0.5 rounds to 6, but 6 - 0.5
    // rounds back to 6, so the unit is at least 1.
    let mut halves = 5_i64;
    for (key, want) in [(Key::ArrowRight, 6), (Key::ArrowLeft, 5)] {
        let mut h = Harness::mono(UVec2::new(200, 40));
        let mut record = |ui: &mut Ui| {
            Slider::new(&mut halves, 0.0..=10.0)
                .id(id)
                .step(0.5)
                .show(ui);
        };
        h.frame(&mut record);
        h.ui_mut().set_focus(id);
        h.key(key);
        h.frame(&mut record);
        assert_eq!(halves, want, "{key:?} on a half step");
    }
}

/// A slider wears the theme's enabled colors on its fill, track and knob, and its disabled ones
/// while disabled.
#[test]
fn a_disabled_slider_wears_the_disabled_colors() {
    use crate::scene::layer::Layer;
    use crate::shape::paint::lowered_brush::LoweredBrush;

    let id = WidgetId::from_hash("slider-colors");
    for disabled in [false, true] {
        let mut h = Harness::mono(UVec2::new(200, 40));
        let mut value = 0.5_f64;
        h.frame(|ui| {
            Slider::new(&mut value, 0.0..=1.0)
                .id(id)
                .disabled(disabled)
                .show(ui);
        });
        let theme = h.ui().theme().slider.clone();
        assert_ne!(theme.enabled, theme.disabled, "premise: the states differ");
        let want = if disabled {
            theme.disabled
        } else {
            theme.enabled
        };
        for (part, color) in [
            ("fill", want.fill),
            ("rest", want.track),
            ("knob", want.knob),
        ] {
            let node = h.node_of(id.with(part)).expect("part recorded").node;
            let fill = h
                .ui()
                .tree(Layer::Main)
                .chrome(node)
                .expect("the part paints chrome")
                .fill;
            let color = color.to_f16();
            assert!(
                matches!(fill, LoweredBrush::Solid(got) if got == color),
                "disabled {disabled}, {part}: {fill:?}, want {color:?}",
            );
        }
    }
}
