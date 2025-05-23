# Contributing to the Project

This document provides guidelines for contributing to the project.

## Coding Conventions

- Follow the existing code style in each file.
- Add comments for complex logic
- Use meaningful variable and function names
- Update the documentation in the [DOC.md](DOC.md) file to reflect current state of the project.
  - Maintain project structure and organization.
  - Ensure that the documentation is clear and concise.
  - Add common terms, definitions and internal design decisions.
- Update [DESIGN.md](DESIGN.md) to reflect current state project vision and design decisions.
- Keep components generated small and focused
- Use asserts to check for invalid states in the code.
- Use asserts to validate inputs to functions.
- Add tests for new features and bug fixes.
  - Use `cargo test` to run the tests.
  - Use `cargo clippy` to run the linter.
  - Use `cargo fmt` to format the code.
  - If there are no rust code changes, skip the `cargo clippy`, `cargo fmt` and `cargo test` steps.
  - Add `Debug` derive to all structs and enums.
- Keep [DESIGN.md](DESIGN.md) and [DOC.md](DOC.md) up to date