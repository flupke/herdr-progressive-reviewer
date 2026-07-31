.PHONY: install

install:
	cargo build --release --locked --bins
	mkdir -p bin
	cp target/release/pr-app bin/pr-app.new
	mv bin/pr-app.new bin/pr-app
	cp target/release/pr-control bin/pr-control.new
	mv bin/pr-control.new bin/pr-control
	herdr plugin link . --enabled
