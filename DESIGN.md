# Palantir - a rust GUI Library: Research & Development

## This document

This document will be continually updated as the project evolves to ensure clarity and alignment for all contributors.

## Goal

The goal of this project is to research and develop a Rust-based Graphical User Interface (GUI) library that is performant, cross-platform, and purely reliant on the Rust compiler without external dependencies. The library will feature a clean, declarative API, similar in style to modern frameworks like SwiftUI, leveraging minimal macro usage for improved readability and maintainability.

---

## Core Requirements

### Declarative API

* The GUI must adopt a declarative programming style, allowing UI definitions through structured, expressive Rust code.
* The code should prioritize readability and clarity, resembling the concise nature of SwiftUI.

### Pure Rust Implementation

* No non-Rust runtime requirements.
* The library ships as a single crate and **does not require** C/C++ tool-chains or system DLLs at runtime.  
* Rust crates from crates.io (e.g. `wgpu`, `winit`, etc.) are allowed; foreign-language bindings are avoided.
* Avoid usage of macros unless strictly necessary to maintain clarity.
* GUI elements and logic must rely solely on Rust compiler capabilities.

### Cross-platform Support

* Must function consistently across multiple operating systems, including Windows, macOS, and Linux.
* Platform-specific details should be abstracted internally within the library.

### Styling and Theming

* Support for per-component styling and global theming.

### Performance

* The GUI should be performant, leveraging Rust’s speed and efficiency.
* Minimize unnecessary redraws and resource usage.

### Testing

* Support headless renderer for layout unit testing.

### Accessibility

* Not considered in initial design.

---

## Proposed Design

* wgpu is rendering backend.

### Declarative GUI Definition

The GUI definitions will use a clear, concise, builder-pattern-inspired API:

```rust
fn main() {
    Gui::new(Window::new("My App", || {
        VStack::new()
            .padding(10)
            .spacing(5)
            .add(Text::new("Hello, world!")
                .font_size(18)
                .color(Color::Blue))
            .add(Button::new("Click me!", || {
                println!("Button clicked!");
            }))
    })).run();
}
```

### Grid Layout Design

The Grid component will explicitly declare child components using column and row indices:

```rust
Grid::new()
    .padding(10)
    .spacing(8)
    .columns([
        Column::fixed(100),
        Column::auto().min_width(50),
    ])
    .rows([
        Row::auto(),
        Row::auto(),
        Row::auto(),
    ])
    .add(
        Text::new("Name:")
            .align(Alignment::Right)
            .grid_pos((0, 0).into()), // Column 0, Row 0
    )
    .add(
        Button::new("Submit", || println!("Submitted!"))
            .padding(5)
            .align(Alignment::Right)
            .grid_pos((0, 1, 2, 1).into()), // Column 0, Row 1, RowSpan 2, ColumnSpan 1
    );
```

### Styling and Theming

Components will support direct styling methods:

```rust
Text::new("Styled Text")
    .font_size(16)
    .color(Color::Red)
    .padding(8)
```

Global theming:

```rust
Theme::new()
    .font("Arial")
    .primary_color(Color::Blue)
    .secondary_color(Color::Gray)
    .apply();
```

### Alignment System

#### VerticalAlignment

* **Top** - The child element is aligned to the top of the parent's layout slot.
* **Center** - The child element is aligned to the center of the parent's layout slot.
* **Bottom** - The child element is aligned to the bottom of the parent's layout slot.
* **Stretch** - The child element stretches to fill the parent's layout slot.

#### HorizontalAlignment

* **Left** - An element aligned to the left of the layout slot for the parent element.
* **Center** - An element aligned to the center of the layout slot for the parent element.
* **Right** - An element aligned to the right of the layout slot for the parent element.
* **Stretch** - An element stretched to fill the entire layout slot of the parent element.

#### Alignment Remarks

* **Stretch** is the default layout behavior.
* Element Height and Width properties that are explicitly set take precedence over the Stretch property value.

```rust
Text::new("Aligned Text")
    .vertical_alignment(VerticalAlignment::Center)
    .horizontal_alignment(HorizontalAlignment::Right)
```

---

## Layout Calculation System

The GUI library will implement a two-pass layout system inspired by WPF (Windows Presentation Foundation), providing flexible and content-aware layouts through separate measure and arrange phases.

### Two-Pass Layout Process

#### 1. Measure Pass (Top-Down)

The measure pass determines the desired size of each element in the UI tree:

- **Parent-to-Child Communication**: Parent elements call the `measure()` method on their children, providing an `available_size` that represents the space the parent can offer.
- **Child Response**: Child elements calculate their `desired_size` based on their content, margins, padding, and constraints (MinWidth, MaxHeight, etc.).
- **Recursive Process**: This process starts from the root element and proceeds down the visual tree recursively.
- **Goal**: Determine how much space each element would like to occupy without assigning final sizes or positions.

#### 2. Arrange Pass (Bottom-Up)

The arrange pass finalizes the size and position of each element:

- **Parent Allocation**: Parent elements call the `arrange()` method on their children, providing a `final_rect` that specifies the exact size and position.
- **Child Positioning**: Child elements use this information to position themselves and arrange their own children.
- **Alignment Consideration**: This phase applies alignment properties (HorizontalAlignment, VerticalAlignment) and handles stretching behavior.
- **Final Layout**: Each element receives its exact size and position on the screen.

```rust
// Auto sizing - fits content
Text::new("Dynamic content")
    .width(SizeMode::Auto)
    .height(SizeMode::Auto)

// Stretching - fills available space
Button::new("Stretch Button", || {})
    .horizontal_alignment(HorizontalAlignment::Stretch)
    .vertical_alignment(VerticalAlignment::Stretch)

// Fixed size - explicit dimensions
Text::new("Fixed size text")
    .width(SizeMode::Fixed(50.0))
    .height(200.0.into())           // either way
```

---

## Next Steps

* Define comprehensive components (Button, TextField, Label, etc.)
* Explore efficient state management and reactive updates.
* Prototype wgpu rendering backend.
* Validate usability and performance metrics.

---


