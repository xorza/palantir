# Focus

v1 ships `focused`, `FocusPolicy`, programmatic `request_focus`,
click-to-focus, eviction-on-removal, escape-to-blur.

## Next

- **Tab cycling.** `Tab` / `Shift+Tab` over the cascade in pre-order,
  skipping non-focusable / disabled. Multi-line editors opt into
  consuming Tab.
- **Focus ring.** Centralized `focused`-state outline shape so a11y /
  high-contrast can boost it.
- **Focus-on-disabled rule.** Going disabled while focused should
  release focus. Pin it.
- **Focus restoration.** Optional remember-and-restore when a focused
  widget vanishes (modal-close → restore caller).
