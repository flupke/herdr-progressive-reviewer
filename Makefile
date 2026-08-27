.PHONY: build check complexity install mutants uninstall

build:
	cargo build --release --locked --bins
	mkdir -p bin
	for binary in reviewer reviewer-control; do \
		cp "target/release/$$binary" "bin/$$binary.new"; \
		mv -f "bin/$$binary.new" "bin/$$binary"; \
	done

check: export RUSTFLAGS = -Dwarnings
check:
	cargo fmt --all --check
	cargo check --workspace
	cargo clippy --workspace --all-targets
	cargo test --workspace --doc
	cargo nextest run --workspace

complexity:
	cccc --lang rust crates | jq -r '[.files[] | .path as $$path | .functions[] | recurse(.children[]?) | select(.cyclomatic > 10 or .cognitive > 15) | { path: $$path, line, name, cognitive, cyclomatic }] | sort_by([-.cyclomatic, -.cognitive, .path, .line]) | ("Cognitive\tCyclomatic\tFunction", (.[] | "\(.cognitive)\t\(.cyclomatic)\t\(.path):\(.line) \(.name)"))'

mutants:
	cargo mutants --workspace --test-workspace=true --test-tool=nextest

install: build
	herdr plugin link . --enabled

uninstall:
	herdr plugin unlink herdr.progressive-reviewer
