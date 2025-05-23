# Palantir Project Documentation

Refer to [README.md](README.md) for high-level overview of the project.
This document provides additional details on the project internal structure and design decisions for contributors.

## Repository Layout

- **palantir** – main GUI crate containing the core framework
- **target** – build artifacts and dependencies
- **DESIGN.md** – architectural decisions and design vision

## Project Structure

### Core Module: `view.rs`

The main module containing the foundational traits and components:

#### Traits
- `View` - Base trait requiring mutable access to styling
- `Stylable` - Fluent API for styling (blanket implemented for all `View` types)
- `ItemsView` - For containers managing multiple children
- `ItemView` - For components containing a single child

#### Components
- `VStack` - Vertical stack container
- `Label` - Text display component
- `Button` - Interactive button component

#### Styling
- `Style` struct with padding, margin, font_size, and color
- `Colors` struct with predefined color constants

### Design Patterns

#### Trait-Based Architecture
The framework uses traits extensively to provide composable functionality:
- Every UI component implements `View`
- Styling is automatically available through blanket implementation
- Container behavior is opt-in through specific traits

#### Builder Pattern
All components use method chaining for configuration:
```rust
VStack::new()
    .padding(10.0)
    .add(Label::from("Text").font_size(18))
```

#### Type Safety
The framework leverages Rust's type system to prevent common UI errors at compile time.

## Development Guidelines

### Adding New Components
1. Implement `View` trait
2. Add `Style` field to struct
3. Implement specific behavior traits (`ItemsView`, `ItemView`, etc.)
4. Styling automatically available via blanket implementation

### Testing
The `lib.rs` contains integration tests demonstrating component usage.

Commit messages are often prompts sent to an AI agent to request a change.
