.PHONY: check install

check: export RUSTFLAGS = -Dwarnings
check:
	cargo fmt --all --check
	cargo check --workspace
	cargo clippy --workspace --all-targets
	cargo test --workspace

install:
	cargo build --release --locked --bins
	mkdir -p bin
	cp target/release/reviewer bin/reviewer.new
	mv bin/reviewer.new bin/reviewer
	cp target/release/reviewer-control bin/reviewer-control.new
	mv bin/reviewer-control.new bin/reviewer-control
	herdr plugin link . --enabled
