# Regression tests from LLM explorations

Add UI regression cases here after reproducing them in a vision session.
Each test should identify the exploration finding it protects, document the
expected behavior, and reproduce the relevant interactions through the real UI.
Keep supporting evidence with the test rather than relying on ignored artifacts
under `target/`.

See the [exploration guide](../../../.agents/wiki/tui-vision.md#llm-directed-exploration)
for commands and capture details. Run the resulting tests with
`nix develop --command make e2e-tui`.
