.PHONY: build dev test test-rust test-python lint fmt clean

build:
	maturin build --release

dev:
	maturin develop

test: test-rust dev test-python

test-rust:
	cargo test -p reconcile-core

test-python:
	pytest tests/python/ -v

lint:
	cargo fmt --check
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all

clean:
	cargo clean
	rm -rf target/ dist/ *.egg-info
