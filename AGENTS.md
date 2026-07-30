# Code style

- Keep crates small, don't hesitate to create tiny ones or split a big one.
- Avoid "functions soup", design types first, then implement their methods.
- Use `#[must_use]` only when ignoring a return value is likely to cause a bug.
  Do not add it to routine getters or to functions that return types that
  already have this attribute.
- Always use the smallest visibility level that permits the required use. Do
  not make an item `pub` when private or `pub(crate)` visibility is sufficient.
