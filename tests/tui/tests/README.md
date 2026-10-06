# Regression tests from LLM explorations

Add UI regression cases here after reproducing them in a vision session. A test drives the
reviewer in a private Herdr (`HerdrTestServer`), never the user's.
Each test should identify the exploration finding it protects, document the
expected behavior, and reproduce the relevant interactions through the real UI.
Keep supporting evidence with the test rather than relying on ignored artifacts
under `target/`.

See the [exploration guide](../../../.agents/wiki/tui-vision.md)
for the vision MCP server's tools. Run the resulting tests with
`nix develop --command make e2e-tui`.
