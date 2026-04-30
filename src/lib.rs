pub mod primitives;
pub mod shape;
pub mod tree;
pub mod ui;
pub mod layout;
pub mod widgets;

pub use primitives::{Color, Corners, Rect, Size, Sizes, Sizing, Spacing, Stroke, Style, WidgetId};
pub use shape::{Shape, ShapeRect};
pub use tree::{LayoutKind, Node, NodeId, Tree};
pub use ui::Ui;
pub use widgets::{Button, HStack, Response, Stack, VStack};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hstack_arranges_two_buttons_side_by_side() {
        let mut ui = Ui::new();
        ui.begin_frame();

        let root = HStack::new("root").show(&mut ui, |ui| {
            Button::new("a").label("Hi").show(ui);
            Button::new("b").label("World").width(100.0).show(ui);
        }).node;

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

        let root = VStack::new("root").show(&mut ui, |ui| {
            Button::new("fixed").height(50.0).show(ui);
            Button::new("filler").height(Sizing::Fill).show(ui);
        }).node;

        let surface = Rect::new(0.0, 0.0, 200.0, 300.0);
        layout::run(&mut ui.tree, root, surface);

        let kids: Vec<_> = ui.tree.children(root).collect();
        let fixed  = ui.tree.node(kids[0]).rect;
        let filler = ui.tree.node(kids[1]).rect;

        assert_eq!(fixed.size.h, 50.0);
        assert_eq!(filler.min.y, 50.0);
        assert_eq!(filler.size.h, 250.0);
    }

    #[test]
    fn shapes_attached_to_button_node() {
        let mut ui = Ui::new();
        ui.begin_frame();
        let mut button_node = None;
        HStack::new("root").show(&mut ui, |ui| {
            button_node = Some(Button::new("only").label("X").show(ui).node);
        });

        let shapes = ui.tree.shapes_of(button_node.unwrap());
        assert_eq!(shapes.len(), 2);
        assert!(matches!(shapes[0], Shape::RoundedRect { .. }));
        assert!(matches!(shapes[1], Shape::Text { .. }));
    }
}
