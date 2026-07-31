# Code style

- Keep crates small, don't hesitate to create tiny ones or split a big one.
- Avoid "functions soup", design types first, then implement their methods.
- Use `#[must_use]` only when ignoring a return value is likely to cause a bug.
  Do not add it to routine getters or to functions that return types that
  already have this attribute.
- Always use the smallest visibility level that permits the required use. Do
  not make an item `pub` when private or `pub(crate)` visibility is sufficient.

# Small feature workflow

For each small feature:

1. Create a fresh jj change before implementation. Use the previous change as
   the fixed point for this feature.
2. Implement and validate the feature.
3. Run `$code-review` against the fixed point. Fix its findings and repeat the
   review until it passes. Follow the skill's repair-loop limit and report any
   findings that remain when the limit is reached.
4. After the review passes, run `$describe-commit` for the change.
5. Keep later user-feedback fixes in the same change. Create another change
   only when the user requests the next feature.
