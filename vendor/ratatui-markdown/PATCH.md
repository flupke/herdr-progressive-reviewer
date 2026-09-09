This is the source and examples from the published ratatui-markdown 0.3.6
crate (https://crates.io/crates/ratatui-markdown/0.3.6), omitting example image
assets and screenshots. It retains its original
manifest and license. It is excluded from the application workspace.

The only source modification is in src/markdown/inline.rs: remove the
spaces the inline-code parser synthesizes before and after backtick spans.
Source whitespace and code styling remain unchanged. Fixing this before
wrapping also keeps punctuation from wrapping because of invented padding.
The dependency's inline_code hook does not cover code inside paragraphs,
headings, lists, quotes and tables, which use this shared parser directly.

Application-level regressions live in crates/markdown-rendering/src/tests.rs.
Remove this patch when an upstream release includes the spacing fix.
