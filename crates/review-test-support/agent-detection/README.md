# Herdr agent detection rules for the tests

Every Herdr test server installs these manifests as local overrides, so the
tests detect agents with the same rules whatever Herdr release runs them. See
[the development guide](../../../docs/development.md#the-herdr-the-tests-run).

They are unmodified copies of `distribution/agent-detection/codex.toml`
(version 2026.10.01.1) and `distribution/agent-detection/claude.toml` (version
2026.09.11.1) from the Herdr repository, https://github.com/herdrdev/herdr, at
commit `07e3840bfb41f433cb7f14b4f93e2ff3fbaac6d7`. Herdr is licensed under the
Apache License, Version 2.0: https://www.apache.org/licenses/LICENSE-2.0
