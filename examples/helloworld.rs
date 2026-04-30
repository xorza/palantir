use palantir::{layout, Button, LayoutKind, Rect, Sizing, Style, Ui, WidgetId};

fn main() {
    let mut ui = Ui::new();
    ui.begin_frame();

    let root = ui.begin_node(
        WidgetId::from_hash("root"),
        Style::default(),
        LayoutKind::HStack,
    );
    Button::new("a").label("Hello").show(&mut ui);
    Button::new("b").label("World").width(Sizing::Fill).show(&mut ui);
    ui.end_node(root);

    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 800.0, 600.0));

    for (i, n) in ui.tree.nodes.iter().enumerate() {
        println!("node {i}: rect={:?} desired={:?}", n.rect, n.desired);
        for s in &ui.tree.shapes[n.shapes_start as usize .. n.shapes_end as usize] {
            println!("  shape: {s:?}");
        }
    }
}
