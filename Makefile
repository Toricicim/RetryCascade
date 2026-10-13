.PHONY: setup build schemas test smoke pilot discovery followup followup-smoke

setup:
	cd python && uv sync --locked --group dev

build:
	cargo build --release --locked -p retrycascade-cli

schemas:
	cd python && uv run --locked python -m retrycascade.v2_config

test:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace --locked
	cd python && uv run --locked pytest

smoke: build
	cd python && uv run --locked python -m retrycascade.study run --preset smoke --output ../output/v2-smoke

pilot: build
	cd python && uv run --locked python -m retrycascade.study run --preset pilot --output ../output/v2-pilot

discovery: build
	cd python && uv run --locked python -m retrycascade.study run --preset discovery --output ../output/v2-discovery

followup: build
	cd python && uv run --locked python -m retrycascade.study run --preset followup --output ../output/v2-followup

followup-smoke: build
	cd python && uv run --locked python -m retrycascade.study run --preset followup-smoke --output ../output/v2-followup-smoke
