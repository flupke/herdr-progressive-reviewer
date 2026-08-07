.PHONY: build check install mutants uninstall

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

mutants:
	cargo mutants --workspace --test-workspace=true --test-tool=nextest

install: build
	herdr plugin link . --enabled

uninstall:
	herdr plugin unlink herdr.progressive-reviewer
