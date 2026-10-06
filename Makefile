.PHONY: build check complexity fmt lint test e2e-tui e2e-explore e2e-explore-deps e2e-explore-judgements explore-gallery explore-page explore-types vision install mutants uninstall

build:
	cargo build --release --locked --bins
	mkdir -p bin
	for binary in reviewer reviewer-control reviewer-mcp; do \
		cp "target/release/$$binary" "bin/$$binary.new"; \
		mv -f "bin/$$binary.new" "bin/$$binary"; \
	done

# The checks come in levels, cheapest first; AGENTS.md says when to run which.
# They all build with the same flags, so they share one build cache.
lint test check e2e-tui e2e-explore: export RUSTFLAGS = -Dwarnings

# Level 1: types and lints of every target, and the complexity gate.
lint: complexity
	cargo clippy --workspace --all-targets

# Level 2, with CRATES: the unit tests of those crates (`make test CRATES="quick-tunnel"`).
# Level 3, without: every unit test of the workspace, doc tests included.
CRATES ?=
test:
ifeq ($(strip $(CRATES)),)
	cargo test --workspace --doc
	cargo nextest run --workspace
else
	cargo nextest run $(foreach crate,$(CRATES),-p $(crate))
endif

# Formats the code, once a feature is complete. No check fails on formatting.
fmt:
	cargo fmt --all
	cargo fmt --manifest-path tests/tui/Cargo.toml

# Every level but `make vision`, in one run.
check: lint
	$(MAKE) test CRATES=
	$(MAKE) e2e-tui
	$(MAKE) e2e-explore

e2e-tui:
	cargo build --locked -p reviewer --bin reviewer
	cargo clippy --locked --manifest-path tests/tui/Cargo.toml --all-targets
	REVIEWER_BIN_PATH="$(CURDIR)/target/debug/reviewer" cargo nextest run --locked --manifest-path tests/tui/Cargo.toml

# The e2e tests of the Explore page (https://github.com/tester-army/e2e). Agent steps replay
# their recordings under tests/explore-page/.e2e/cache; a new or stale step goes to the model and
# the cache is updated (docs/development.md). E2E_ARGS go to `e2e run`.
EXPLORE_E2E = tests/explore-page

$(EXPLORE_E2E)/node_modules/.installed: $(EXPLORE_E2E)/package-lock.json
	cd $(EXPLORE_E2E) && npm ci --no-audit --no-fund
	touch $@

# Installs the npm packages only, for the MCP server (tests/explore-page/mcp.sh).
e2e-explore-deps: $(EXPLORE_E2E)/node_modules/.installed

# The rule on judgements in the e2e tests (docs/development.md, "e2e tests").
e2e-explore-judgements:
	node --test --test-reporter=dot $(EXPLORE_E2E)/judgements.test.ts
	node $(EXPLORE_E2E)/judgements.ts

# Checks the page's client against the TypeScript declarations of its socket's messages, then
# runs the e2e tests.
e2e-explore: e2e-explore-judgements e2e-explore-deps
	cd $(EXPLORE_E2E) && node_modules/.bin/tsc -p tsconfig.client.json
	cargo build --locked -p review-explore-page-server
	$(EXPLORE_E2E)/run.sh $(E2E_ARGS)

# Screenshots of every state of the Explore page, at each width and theme, with a contact sheet
# (docs/development.md, "Screenshot gallery"). GALLERY_DIR, GALLERY_COMPARE, GALLERY_WIDTHS and
# GALLERY_STATES go to the script through the environment.
explore-gallery: e2e-explore-deps
	cargo build --locked -p review-explore-page-server
	node $(EXPLORE_E2E)/gallery/gallery.ts

# Serve the Explore page alone, with a fixed question. Templates and assets are read from disk,
# and an open page reloads when one changes. EXPLORE_PAGE_ARGS go to the server
# (`EXPLORE_PAGE_ARGS='--data rich'` serves the gallery's long round).
explore-page:
	cargo run --locked -p review-explore-page-server -- --port 8790 --token dev --dev crates/review-explore-page $(EXPLORE_PAGE_ARGS)

# Writes the TypeScript declarations of the Explore page's socket messages from their Rust
# types (crates/review-explore-page/assets/client/types.ts); `make check` fails while the
# committed file differs from them.
explore-types:
	EXPLORE_TYPES=write cargo test --locked -p review-explore-page typescript

vision:
	cargo build --locked -p reviewer --bin reviewer --bin reviewer-control
	REVIEWER_BIN_PATH="$(CURDIR)/target/debug/reviewer" cargo run --locked --manifest-path tests/tui/Cargo.toml --bin reviewer-vision -- $(VISION_ARGS)

complexity:
	@report="$$(cccc --lang rust crates | jq -r '[.files[] | .path as $$path | .functions[] | recurse(.children[]?) | select(.cyclomatic > 10 or .cognitive > 15) | { path: $$path, line, name, cognitive, cyclomatic }] | sort_by([-.cyclomatic, -.cognitive, .path, .line]) | if length == 0 then empty else ("Cognitive\tCyclomatic\tFunction", (.[] | "\(.cognitive)\t\(.cyclomatic)\t\(.path):\(.line) \(.name)")) end')" || exit; \
	if [ -n "$$report" ]; then printf '%s\n' "$$report"; exit 1; fi

mutants:
	cargo mutants --workspace --test-workspace=true --test-tool=nextest

# Builds and installs without running the checks: run `make check` before installing.
install: build
	bin/reviewer-control mcp-install
	herdr plugin link . --enabled

uninstall:
	herdr plugin unlink herdr.progressive-reviewer
