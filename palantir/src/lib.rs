pub mod view;

use view::View;
use view::Stylable;
use view::VStack;
use view::Label;
use view::Button;
use view::ItemsView;
use view::ItemView;
use view::Style;
use view::Colors;


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        VStack::new()
            .padding(10.0)
            .margin(5.0)
            .add(
                Label::from("Hello, world!")
                    .font_size(18)
                    .color(Colors::BLUE),
            )
            .add(
                Button::new()
                    .item(Label::from("Hello, world!"))
                    .onclick(|| {
                        println!("Button clicked!");
                    }),
            );
    }
}
