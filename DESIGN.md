# Rust GUI Library: Research & Development

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

* No external dependencies apart from the Rust standard library.
* Avoid usage of macros unless strictly necessary to maintain clarity.
* GUI elements and logic must rely solely on Rust compiler capabilities.

### Cross-platform Support

* Must function consistently across multiple operating systems, including Windows, macOS, and Linux.
* Platform-specific details should be abstracted internally within the library.

### Styling and Theming

* Robust support for per-component styling and global theming.
* Styles and themes should be intuitive to define and apply.

### Performance

* The GUI should be performant, leveraging Rust’s speed and efficiency.
* Minimize unnecessary redraws and resource usage.

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
    .column(Column::fixed(100))
    .column(Column::auto().min_width(50))
    .row(Row::auto())
    .row(Row::auto())
    .add(
        (0, 1).into(),  // Column index and span
        0.into(),       // Row index
        Text::new("Name:").align(Alignment::Right),
    )
    .add(
        1.into(),       // Column index
        (1, 2).into(),  // Row index and span
        Button::new("Submit", || println!("Submitted!"))
            .padding(5)
            .align(Alignment::Right),
    );
```

#### Internal Representations

Internally, positioning and span are encapsulated as follows:

```rust
struct ColumnPosition {
    start: usize,
    span: usize,
}

struct RowPosition {
    start: usize,
    span: usize,
}

impl From<(usize, usize)> for ColumnPosition {
    fn from(value: (usize, usize)) -> Self {
        ColumnPosition { start: value.0, span: value.1 }
    }
}
impl From<usize> for ColumnPosition {
    fn from(value: usize) -> Self {
        ColumnPosition { start: value.0, span: 1 }
    }
}

// same for rows
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

```rust
trait Measurable {
    fn measure(&mut self, available_size: Size) -> Size;
}

impl Measurable for Text {
    fn measure(&mut self, available_size: Size) -> Size {
        // Calculate text dimensions based on font, content, and available space
        let text_metrics = self.calculate_text_metrics(available_size);
        Size::new(
            text_metrics.width.min(available_size.width),
            text_metrics.height.min(available_size.height)
        )
    }
}
```

#### 2. Arrange Pass (Bottom-Up)

The arrange pass finalizes the size and position of each element:

- **Parent Allocation**: Parent elements call the `arrange()` method on their children, providing a `final_rect` that specifies the exact size and position.
- **Child Positioning**: Child elements use this information to position themselves and arrange their own children.
- **Alignment Consideration**: This phase applies alignment properties (HorizontalAlignment, VerticalAlignment) and handles stretching behavior.
- **Final Layout**: Each element receives its exact size and position on the screen.

```rust
trait Arrangeable {
    fn arrange(&mut self, final_rect: Rect);
}

impl Arrangeable for Button {
    fn arrange(&mut self, final_rect: Rect) {
        self.bounds = final_rect;
        // Apply alignment and arrange internal content
        self.arrange_content(final_rect);
    }
}
```

### Layout Behavior Based on Sizing and Alignment

The layout system supports different sizing behaviors determined during the measure and arrange passes:

#### Auto Sizing
When width or height is set to `Auto`, elements size themselves to fit their content:

```rust
Text::new("Dynamic content")
    .width(SizeMode::Auto)
    .height(SizeMode::Auto)
```

#### Stretching
When alignment is set to `Stretch` and no explicit size is provided, elements expand to fill available space:

```rust
Button::new("Stretch Button", || {})
    .horizontal_alignment(HorizontalAlignment::Stretch)
    .vertical_alignment(VerticalAlignment::Stretch)
```

#### Fixed Size
Explicit width and height values force elements to use exact dimensions:

```rust
Text::new("Fixed size text")
    .width(200.0.into())           // SizeMode from f32
    .height(SizeMode::Fixed(50.0))
```

### Layout Engine Implementation

```rust
pub struct LayoutEngine;

impl LayoutEngine {
    pub fn layout(root: &mut dyn UIElement, available_size: Size) {
        // Measure pass: determine desired sizes
        let desired_size = root.measure(available_size);
        
        // Arrange pass: assign final positions and sizes
        let final_rect = Rect::new(0.0, 0.0, desired_size.width, desired_size.height);
        root.arrange(final_rect);
    }
}

pub trait UIElement: Measurable + Arrangeable {
    fn get_children(&mut self) -> &mut [Box<dyn UIElement>];
    fn get_desired_size(&self) -> Size;
    fn get_bounds(&self) -> Rect;
}
```


### Custom Layout Support

The system provides mechanisms for custom layout behaviors through override methods:

```rust
pub trait CustomLayout: UIElement {
    fn measure_override(&mut self, available_size: Size) -> Size {
        // Default implementation calls standard measure
        self.measure(available_size)
    }
    
    fn arrange_override(&mut self, final_rect: Rect) {
        // Default implementation calls standard arrange
        self.arrange(final_rect)
    }
}
```

### Performance Considerations

- **Layout Invalidation**: Only re-layout when necessary (content changes, size changes, etc.)
- **Incremental Updates**: Support for partial layout updates when only specific elements change
- **Caching**: Cache measurement results when possible to avoid redundant calculations

```rust
pub struct LayoutCache {
    last_available_size: Option<Size>,
    cached_desired_size: Option<Size>,
    is_valid: bool,
}

impl LayoutCache {
    pub fn invalidate(&mut self) {
        self.is_valid = false;
    }
    
    pub fn get_cached_size(&self, available_size: Size) -> Option<Size> {
        if self.is_valid && self.last_available_size == Some(available_size) {
            self.cached_desired_size
        } else {
            None
        }
    }
}
```

---

## Next Steps

* Define comprehensive components (Button, TextField, Label, etc.)
* Explore efficient state management and reactive updates.
* Prototype wgpu rendering backend.
* Validate usability and performance metrics.

---


