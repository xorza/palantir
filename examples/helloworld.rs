use palantir::{layout, Button, HStack, Rect, Sizing, Ui};


fn main() {
    let mut ui = Ui::new();
    ui.begin_frame();

    let root = HStack::new().show(&mut ui, |ui| {
        Button::new().label("Hello").show(ui);
        Button::new().label("World").size((Sizing::Fill, Sizing::Hug)).show(ui);
    }).node;

    layout::run(&mut ui.tree, root, Rect::new(0.0, 0.0, 800.0, 600.0));

    for (i, n) in ui.tree.nodes.iter().enumerate() {
        println!("node {i}: rect={:?} desired={:?}", n.rect, n.desired);
        for s in &ui.tree.shapes[n.shapes_start as usize .. n.shapes_end as usize] {
            println!("  shape: {s:?}");
        }
    }
}
