use crate::internals::layout_rig::{LayoutRig, NodeRecorder};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use glam::Vec2;

const SURFACE: Size = Size::new(400.0, 300.0);

/// A 200 × 100 scroller holding a vertical viewport over content `content_h` tall and the bar
/// overlay over it: an 8 px bar in an 8 px right gutter, a 20 px shortest thumb, scrolled 120 px
/// down.
fn scroller(rec: &mut NodeRecorder<'_>, content_h: f32) {
    let fill = (Sizing::FILL, Sizing::FILL);
    rec.node("scroller", Widget::zstack().size((200.0, 100.0)), |rec| {
        let view = Widget::scroll(ScrollAxes::VERTICAL).size(fill);
        rec.node("view", view, |rec| {
            rec.leaf("content", Widget::leaf().size((180.0, content_h)));
        });
        let def = ScrollbarsDef {
            content: WidgetId::from_hash("view"),
            offset: Vec2::new(0.0, 120.0),
            zoom: 1.0,
            axes: ScrollAxes::VERTICAL,
            reserve: Spacing::new(0.0, 0.0, 8.0, 0.0),
            padding: Spacing::ZERO,
            bar_thickness: 8.0,
            min_thumb: 20.0,
        };
        rec.scrollbars("bars", Widget::scrollbars().size(fill), def, |rec| {
            for bar in ["v-track", "v-thumb", "h-track", "h-thumb"] {
                rec.leaf(bar, Widget::leaf());
            }
        });
    });
}

fn rect(min: (f32, f32), size: (f32, f32)) -> Rect {
    Rect {
        min: Vec2::new(min.0, min.1),
        size: Size::new(size.0, size.1),
    }
}

/// The vertical bar's track runs the overlay's whole height in the right 8 px, and its thumb is
/// the share of the track the viewport shows, placed by the scroll's share of its travel; the
/// axis that does not pan gets no bar, its leaves zero-size at the overlay's origin. Over 400 px
/// of content in a 100 px track the thumb is 100 / 400 × 100 = 25 px, its travel 75, and 120 of
/// the 300 px the content can scroll puts it 0.4 × 75 = 30 px down.
///
/// The overlay arranges from the viewport's measure, not its own slot: content grown to 600 px
/// with the slot unchanged makes the thumb 100 / 600 × 100 = 16.7, held to the 20 px shortest,
/// and 120 of 500 puts it 0.24 × 80 = 19.2, rounded to 19, px down. Content that fits takes the
/// bar away.
#[test]
fn bars_follow_the_viewports_content() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| scroller(rec, 400.0));
    assert_eq!(rig.rect("bars"), rect((0.0, 0.0), (200.0, 100.0)));
    assert_eq!(rig.rect("v-track"), rect((192.0, 0.0), (8.0, 100.0)));
    assert_eq!(rig.rect("v-thumb"), rect((192.0, 30.0), (8.0, 25.0)));
    for absent in ["h-track", "h-thumb"] {
        assert_eq!(rig.rect(absent), rect((0.0, 0.0), (0.0, 0.0)), "{absent}");
    }

    rig.run(SURFACE, |rec| scroller(rec, 600.0));
    assert_eq!(rig.rect("v-track"), rect((192.0, 0.0), (8.0, 100.0)));
    assert_eq!(
        rig.rect("v-thumb"),
        rect((192.0, 19.0), (8.0, 20.0)),
        "the thumb moves with no change to the overlay's slot"
    );

    rig.run(SURFACE, |rec| scroller(rec, 80.0));
    for absent in ["v-track", "v-thumb", "h-track", "h-thumb"] {
        assert_eq!(
            rig.rect(absent),
            rect((0.0, 0.0), (0.0, 0.0)),
            "{absent}: content fits"
        );
    }
}
