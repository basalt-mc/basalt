.PHONY: build test check fmt clippy deny codegen coverage

## Build all crates
build:
	cargo build --all-targets

## Run all tests
test:
	cargo test --all-features

## Run fmt + clippy + test
check: fmt clippy test

## Check formatting
fmt:
	cargo fmt --all --check

## Run clippy lints
clippy:
	cargo clippy --all-targets --all-features -- -D warnings

## Run cargo-deny (advisories + licenses)
deny:
	cargo deny check

## Generate protocol packets from minecraft-data
codegen:
	cargo run --package xtask -- codegen
	cargo fmt --all

## Run coverage report locally
coverage:
	cargo llvm-cov --all-features --fail-under-lines 90 --ignore-filename-regex "(examples|packets/)"
