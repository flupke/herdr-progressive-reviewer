.PHONY: build check complexity e2e-tui e2e-explore e2e-explore-deps explore-page vision install mutants uninstall

build:
	cargo build --release --locked --bins
	mkdir -p bin
	for binary in reviewer reviewer-control reviewer-mcp; do \
		cp "target/release/$$binary" "bin/$$binary.new"; \
		mv -f "bin/$$binary.new" "bin/$$binary"; \
	done

check: export RUSTFLAGS = -Dwarnings
check: complexity
	cargo fmt --all --check
	cargo check --workspace
	cargo clippy --workspace --all-targets
	cargo test --workspace --doc
	cargo nextest run --workspace
	$(MAKE) e2e-tui
	$(MAKE) e2e-explore

e2e-tui:
	cargo build --locked -p reviewer --bin reviewer
	cargo fmt --manifest-path tests/tui/Cargo.toml --check
	cargo clippy --locked --manifest-path tests/tui/Cargo.toml --all-targets
	REVIEWER_BIN_PATH="$(CURDIR)/target/debug/reviewer" cargo nextest run --locked --manifest-path tests/tui/Cargo.toml

# The e2e tests of the Explore page (https://github.com/tester-army/e2e). Agent steps replay
# their recordings under tests/explore-page/.e2e/cache; a new or stale step goes to the model and
# the cache is updated (docs/development.md).
EXPLORE_E2E = tests/explore-page

$(EXPLORE_E2E)/node_modules/.installed: $(EXPLORE_E2E)/package-lock.json
	cd $(EXPLORE_E2E) && npm ci --no-audit --no-fund
	touch $@

# Installs the npm packages only, for the MCP server (tests/explore-page/mcp.sh).
e2e-explore-deps: $(EXPLORE_E2E)/node_modules/.installed

e2e-explore: e2e-explore-deps
	cargo build --locked -p review-explore-page-server
	$(EXPLORE_E2E)/run.sh

# Serve the Explore page alone, with a fixed question. Templates and assets are read from disk,
# and an open page reloads when one changes.
explore-page:
	cargo run --locked -p review-explore-page-server -- --port 8790 --token dev --dev crates/review-explore-page

vision:
	cargo build --locked -p reviewer --bin reviewer
	REVIEWER_BIN_PATH="$(CURDIR)/target/debug/reviewer" cargo run --locked --manifest-path tests/tui/Cargo.toml --bin reviewer-vision -- $(VISION_ARGS)

complexity:
	@report="$$(cccc --lang rust crates | jq -r '[.files[] | .path as $$path | .functions[] | recurse(.children[]?) | select(.cyclomatic > 10 or .cognitive > 15) | { path: $$path, line, name, cognitive, cyclomatic }] | sort_by([-.cyclomatic, -.cognitive, .path, .line]) | if length == 0 then empty else ("Cognitive\tCyclomatic\tFunction", (.[] | "\(.cognitive)\t\(.cyclomatic)\t\(.path):\(.line) \(.name)")) end')" || exit; \
	if [ -n "$$report" ]; then printf '%s\n' "$$report"; exit 1; fi

mutants:
	cargo mutants --workspace --test-workspace=true --test-tool=nextest

# Only a build that passes every check is installed.
install: check build
	bin/reviewer-control mcp-install
	herdr plugin link . --enabled

uninstall:
	herdr plugin unlink herdr.progressive-reviewer
