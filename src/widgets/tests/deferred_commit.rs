//! The deferred-commit contract every gesture-driven value widget keeps: a caller that re-seeds
//! its draft each frame and adopts it only on `committed` sees one commit per gesture, carrying
//! the final value.

use crate::host::harness::Harness;
use crate::host::harness::internals::Passes;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widgets::color_field::ColorField;
use crate::widgets::color_strip::ColorStrip;
use crate::widgets::drag_value::DragValue;
use crate::widgets::slider::Slider;
use glam::{UVec2, Vec2};

const ID: &str = "deferred-commit";

/// A caller that keeps a canonical value, shows the widget over a draft of it each record pass,
/// and adopts the draft only on `committed`, as an undo-aware app does.
trait DeferredCaller {
    /// One frame, one [`ValueResponse`] per record pass.
    fn frame(&mut self, h: &mut Harness) -> Passes<ValueResponse>;
    /// The canonical value, as the row reads it.
    fn reading(&self) -> Vec2;
}

#[derive(Debug)]
struct Deferred<T> {
    canonical: T,
    show: fn(&mut Ui, &mut T) -> ValueResponse,
    read: fn(&T) -> Vec2,
}

impl<T: Clone> DeferredCaller for Deferred<T> {
    fn frame(&mut self, h: &mut Harness) -> Passes<ValueResponse> {
        let Self {
            canonical, show, ..
        } = self;
        h.frame_passes(|ui| {
            let mut draft = canonical.clone();
            let edges = show(ui, &mut draft);
            if edges.committed {
                *canonical = draft;
            }
            edges
        })
    }

    fn reading(&self) -> Vec2 {
        (self.read)(&self.canonical)
    }
}

/// One widget under the contract: its window, its gesture, and the values a deferred caller ends
/// on. `press_writes` is false for a widget a press alone leaves, as a drag value's scrub starts
/// past the drag threshold; `clicked` is `None` for one a click does not write.
#[derive(Debug)]
struct Row {
    label: &'static str,
    surface: UVec2,
    caller: fn() -> Box<dyn DeferredCaller>,
    start: Vec2,
    press: Vec2,
    drags: [Vec2; 2],
    press_writes: bool,
    dragged: Vec2,
    clicked: Option<Vec2>,
}

fn slider() -> Box<dyn DeferredCaller> {
    Box::new(Deferred {
        canonical: 0.0_f64,
        show: |ui, value| {
            Slider::new(value, 0.0..=1.0)
                .size((Sizing::fixed(118.0), Sizing::fixed(18.0)))
                .id(WidgetId::from_hash(ID))
                .show(ui)
        },
        read: |value| Vec2::new(*value as f32, 0.0),
    })
}

fn drag_value() -> Box<dyn DeferredCaller> {
    Box::new(Deferred {
        canonical: 10.0_f64,
        show: |ui, value| {
            DragValue::new(value)
                .editable(false)
                .speed(1.0)
                .decimals(2)
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .id(WidgetId::from_hash(ID))
                .show(ui)
        },
        read: |value| Vec2::new(*value as f32, 0.0),
    })
}

fn color_field() -> Box<dyn DeferredCaller> {
    let canonical = ColorCoords::default()
        .with_hue(0.3)
        .with_saturation(0.2)
        .with_value(0.2);
    Box::new(Deferred {
        canonical,
        show: |ui, coords| ColorField::new(coords).id(WidgetId::from_hash(ID)).show(ui),
        read: |coords| Vec2::new(coords.saturation(), coords.value()),
    })
}

fn alpha_strip() -> Box<dyn DeferredCaller> {
    Box::new(Deferred {
        canonical: RgbaF32::hex(0x4cd3ff).with_alpha(1.0),
        show: |ui, color| {
            ColorStrip::alpha(color)
                .id(WidgetId::from_hash(ID))
                .show(ui)
        },
        read: |color| Vec2::new(color.a, 0.0),
    })
}

/// A drag writes on every frame and commits once, on the release, in one record pass; a
/// deferred caller holds its value until then, takes the final one, and reads no signal after.
/// A click commits the value its press wrote. The slider's track runs 100 px from x 9, so x 59,
/// 79 and 89 are 0.5, 0.7 and 0.8. The drag value scrubs 1 a pixel from 10: 20 and then 25 px
/// right are 30 and 35. The 208 × 160 field reads saturation across and value up: (104, 80) is
/// (0.5, 0.5), (156, 120) is (0.75, 0.25) and (156, 40) is (0.75, 0.75). The 208 px alpha bar
/// reads x / 208: 52, 104 and 156 are 0.25, 0.5 and 0.75.
#[test]
fn a_gesture_commits_once_with_its_final_value() {
    let rows = [
        Row {
            label: "slider",
            surface: UVec2::new(118, 18),
            caller: slider,
            start: Vec2::ZERO,
            press: Vec2::new(59.0, 9.0),
            drags: [Vec2::new(79.0, 9.0), Vec2::new(89.0, 9.0)],
            press_writes: true,
            dragged: Vec2::new(0.8, 0.0),
            clicked: Some(Vec2::new(0.5, 0.0)),
        },
        Row {
            label: "drag value",
            surface: UVec2::new(300, 100),
            caller: drag_value,
            start: Vec2::new(10.0, 0.0),
            press: Vec2::new(50.0, 20.0),
            drags: [Vec2::new(70.0, 20.0), Vec2::new(75.0, 20.0)],
            press_writes: false,
            dragged: Vec2::new(35.0, 0.0),
            clicked: None,
        },
        Row {
            label: "color field",
            surface: UVec2::new(208, 160),
            caller: color_field,
            start: Vec2::new(0.2, 0.2),
            press: Vec2::new(104.0, 80.0),
            drags: [Vec2::new(156.0, 120.0), Vec2::new(156.0, 40.0)],
            press_writes: true,
            dragged: Vec2::new(0.75, 0.75),
            clicked: Some(Vec2::new(0.5, 0.5)),
        },
        Row {
            label: "alpha strip",
            surface: UVec2::new(208, 14),
            caller: alpha_strip,
            start: Vec2::new(1.0, 0.0),
            press: Vec2::new(52.0, 7.0),
            drags: [Vec2::new(104.0, 7.0), Vec2::new(156.0, 7.0)],
            press_writes: true,
            dragged: Vec2::new(0.75, 0.0),
            clicked: Some(Vec2::new(0.25, 0.0)),
        },
    ];
    for row in rows {
        let label = row.label;

        let mut h = Harness::mono(row.surface);
        let mut caller = (row.caller)();
        caller.frame(&mut h);
        h.press(row.press, PointerButton::Left);
        caller.frame(&mut h).a().assert_signals(
            row.press_writes,
            false,
            &format!("{label}: press"),
        );
        // `changed` is a level: a pointer held still re-writes the draft the caller re-seeded.
        caller.frame(&mut h).a().assert_signals(
            row.press_writes,
            false,
            &format!("{label}: held still"),
        );
        for at in row.drags {
            h.drag_to(at);
            caller
                .frame(&mut h)
                .a()
                .assert_signals(true, false, &format!("{label}: drag to {at}"));
            assert_eq!(caller.reading(), row.start, "{label}: no adoption mid-drag");
        }
        h.release(PointerButton::Left);
        let released = caller.frame(&mut h);
        assert!(released.a().committed, "{label}: the release commits");
        assert_eq!(released.count_where(|e| e.committed), 1, "{label}: once");
        assert_eq!(caller.reading(), row.dragged, "{label}: the final value");
        caller.frame(&mut h).a().assert_signals(
            false,
            false,
            &format!("{label}: no residual signals"),
        );
        assert_eq!(caller.reading(), row.dragged, "{label}");

        let Some(clicked) = row.clicked else {
            continue;
        };
        let mut h = Harness::mono(row.surface);
        let mut caller = (row.caller)();
        caller.frame(&mut h);
        h.press(row.press, PointerButton::Left);
        caller
            .frame(&mut h)
            .a()
            .assert_signals(true, false, &format!("{label}: a click's press"));
        assert_eq!(caller.reading(), row.start, "{label}");
        h.release(PointerButton::Left);
        let released = caller.frame(&mut h);
        assert_eq!(released.count_where(|e| e.committed), 1, "{label}: a click");
        assert_eq!(
            caller.reading(),
            clicked,
            "{label}: the value the click wrote"
        );
        caller
            .frame(&mut h)
            .a()
            .assert_signals(false, false, &format!("{label}: after the click"));
    }
}
