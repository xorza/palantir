
mod elements;
mod style;
mod utils;
mod view;

pub use elements::*;
pub use style::*;
pub use utils::*;
pub use view::*;

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn it_works() {
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
                    .item(Label::from("Hello, world!"))
                    .onclick(|| {
                        println!("Button clicked!");
                    }),
            );
    }
}
