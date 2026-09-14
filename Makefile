.PHONY: build check complexity install mutants uninstall

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

complexity:
	@report="$$(cccc --lang rust crates | jq -r '[.files[] | .path as $$path | .functions[] | recurse(.children[]?) | select(.cyclomatic > 10 or .cognitive > 15) | { path: $$path, line, name, cognitive, cyclomatic }] | sort_by([-.cyclomatic, -.cognitive, .path, .line]) | if length == 0 then empty else ("Cognitive\tCyclomatic\tFunction", (.[] | "\(.cognitive)\t\(.cyclomatic)\t\(.path):\(.line) \(.name)")) end')" || exit; \
	if [ -n "$$report" ]; then printf '%s\n' "$$report"; exit 1; fi

mutants:
	cargo mutants --workspace --test-workspace=true --test-tool=nextest

install: build
	herdr plugin link . --enabled

uninstall:
	herdr plugin unlink herdr.progressive-reviewer
