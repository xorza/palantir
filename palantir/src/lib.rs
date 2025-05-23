#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unreachable_code)]


mod elements;
mod layout;
mod style;
mod utils;
mod view;

pub use elements::*;
pub use layout::*;
pub use style::*;
pub use utils::*;
pub use view::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_compiles() {
        VStack::default()
            .padding(10.0)
            .margin(5.0)
            .add(
                Label::from("Hello, world!")
                    .font_size(18)
                    .color(Colors::BLUE),
            )
            .add(
                Button::default()
                    .background_color(Colors::RED)
                    .item(
                        Label::from("Hello, world!")
                            .v_align(Align::Center)
                            .h_align(Align::Center)
                            .color(Colors::WHITE),
                    )
                    .onclick(|| {
                        println!("Button clicked!");
                    }),
            );
    }
}
