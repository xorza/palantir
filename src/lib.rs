pub mod input;
pub mod layout;
pub mod primitives;
pub mod renderer;
pub mod shape;
pub mod tree;
pub mod ui;
pub mod widgets;

pub use input::{InputEvent, InputState, PointerButton, PointerState, ResponseState};
pub use primitives::{
    Color, Corners, Rect, Size, Sizes, Sizing, Spacing, Stroke, Style, Visuals, WidgetId,
};
pub use shape::{Shape, ShapeRect};
pub use tree::{LayoutKind, Node, NodeId, Tree};
pub use ui::Ui;
pub use widgets::{Button, ButtonStyle, HStack, Response, Stack, VStack};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hstack_arranges_two_buttons_side_by_side() {
        let mut ui = Ui::new();
        ui.begin_frame();

        let root = HStack::new()
            .show(&mut ui, |ui| {
                Button::new().label("Hi").show(ui);
                Button::new()
                    .label("World")
                    .size((100.0, Sizing::Hug))
                    .show(ui);
            })
            .node;

        let surface = Rect::new(0.0, 0.0, 800.0, 600.0);
        layout::run(&mut ui.tree, root, surface);

        assert_eq!(ui.tree.node(root).rect, surface);

        let kids: Vec<_> = ui.tree.children(root).collect();
        assert_eq!(kids.len(), 2);

        let a = ui.tree.node(kids[0]).rect;
        assert_eq!(a.min.x, 0.0);
        assert_eq!(a.min.y, 0.0);
        assert_eq!(a.size.w, 32.0);
        assert_eq!(a.size.h, 32.0);

        let b = ui.tree.node(kids[1]).rect;
        assert_eq!(b.min.x, 32.0);
        assert_eq!(b.size.w, 100.0);
        assert_eq!(b.size.h, 32.0);
    }

    #[test]
    fn vstack_with_fill_distributes_remainder() {
        let mut ui = Ui::new();
        ui.begin_frame();

        let root = VStack::new()
            .show(&mut ui, |ui| {
                Button::new().size((Sizing::Hug, 50.0)).show(ui);
                Button::new().size((Sizing::Hug, Sizing::Fill)).show(ui);
            })
            .node;

        let surface = Rect::new(0.0, 0.0, 200.0, 300.0);
        layout::run(&mut ui.tree, root, surface);

        let kids: Vec<_> = ui.tree.children(root).collect();
        let fixed = ui.tree.node(kids[0]).rect;
        let filler = ui.tree.node(kids[1]).rect;

        assert_eq!(fixed.size.h, 50.0);
        assert_eq!(filler.min.y, 50.0);
        assert_eq!(filler.size.h, 250.0);
    }

    #[test]
    fn duplicate_widget_id_traces_but_does_not_panic() {
        let mut ui = Ui::new();
        ui.begin_frame();
        HStack::new().show(&mut ui, |ui| {
            Button::with_id("dup").show(ui);
            Button::with_id("dup").show(ui);
        });
        assert_eq!(ui.tree.nodes.len(), 3);
    }

    #[test]
    fn shapes_attached_to_button_node() {
        let mut ui = Ui::new();
        ui.begin_frame();
        let mut button_node = None;
        HStack::new().show(&mut ui, |ui| {
            button_node = Some(Button::new().label("X").show(ui).node);
        });

        let shapes = ui.tree.shapes_of(button_node.unwrap());
        assert_eq!(shapes.len(), 2);
        assert!(matches!(shapes[0], Shape::RoundedRect { .. }));
        assert!(matches!(shapes[1], Shape::Text { .. }));
    }

    #[test]
    fn input_state_press_release_emits_click() {
        // Drive the input state machine without any windowing toolkit. Two frames:
        // frame 1 lays out a button so its rect lands in `last_rects`; then a
        // press+release pair over its rect produces `clicked = true` on frame 2.
        use glam::Vec2;
        let mut ui = Ui::new();

        // Frame 1: build, layout, end_frame to populate last_rects.
        ui.begin_frame();
        let root = HStack::new()
            .show(&mut ui, |ui| {
                Button::with_id("target")
                    .label("hi")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui);
            })
            .node;
        let surface = Rect::new(0.0, 0.0, 200.0, 80.0);
        layout::run(&mut ui.tree, root, surface);
        ui.end_frame();

        // Press inside the button, release inside.
        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0)));
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

        // Frame 2: rebuild; widgets should observe the click in build_ui.
        ui.begin_frame();
        let mut got_click = false;
        HStack::new().show(&mut ui, |ui| {
            let r = Button::with_id("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui);
            got_click = r.clicked();
        });
        assert!(got_click, "press+release inside button rect should click");

        // Click does not stick: next frame without input must clear it.
        let root2 = ui.root();
        layout::run(&mut ui.tree, root2, surface);
        ui.end_frame();
        ui.begin_frame();
        let mut still_clicking = false;
        HStack::new().show(&mut ui, |ui| {
            still_clicking = Button::with_id("target")
                .label("hi")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui)
                .clicked();
        });
        assert!(!still_clicking, "click is one-shot");
    }

    #[test]
    fn input_state_release_outside_does_not_click() {
        use glam::Vec2;
        let mut ui = Ui::new();
        ui.begin_frame();
        let root = HStack::new()
            .show(&mut ui, |ui| {
                Button::with_id("target")
                    .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                    .show(ui);
            })
            .node;
        layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 400.0, 80.0));
        ui.end_frame();

        ui.on_input(InputEvent::PointerMoved(Vec2::new(50.0, 20.0))); // inside
        ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
        ui.on_input(InputEvent::PointerMoved(Vec2::new(300.0, 20.0))); // outside
        ui.on_input(InputEvent::PointerReleased(PointerButton::Left));

        ui.begin_frame();
        let mut got_click = false;
        HStack::new().show(&mut ui, |ui| {
            got_click = Button::with_id("target")
                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                .show(ui)
                .clicked();
        });
        assert!(
            !got_click,
            "release outside the original widget cancels click"
        );
    }
}
