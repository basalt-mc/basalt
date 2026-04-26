.PHONY: build test check fmt clippy deny codegen recipes coverage release vanilla-start vanilla-stop vanilla-logs vanilla-attach

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

## Generate recipe data from minecraft-data
recipes:
	cargo run --package xtask -- recipes
	cargo fmt --all

## Run coverage report locally
coverage:
	cargo llvm-cov --all-features --fail-under-lines 90 --ignore-filename-regex "(examples|packets/)"

## Cut a release: bump version, update changelog, commit, tag
## Usage: make release VERSION=0.2.0
release:
	@test -n "$(VERSION)" || (echo "Usage: make release VERSION=x.y.z" && exit 1)
	@echo "Releasing v$(VERSION)..."
	sed -i.bak 's/^version = ".*"/version = "$(VERSION)"/' Cargo.toml && rm Cargo.toml.bak
	sed -i.bak -E 's/^(basalt-[a-z]+ = \{ path = "crates\/basalt-[a-z]+", version = ")[^"]*"/\1$(VERSION)"/' Cargo.toml && rm Cargo.toml.bak
	cargo check --workspace --lib --bins --examples
	git-cliff --tag "v$(VERSION)" --output CHANGELOG.md
	git add Cargo.toml Cargo.lock CHANGELOG.md
	git commit -m "chore(workspace): release v$(VERSION)"
	git tag "v$(VERSION)"
	@echo "Done. Run 'git push && git push --tags' to trigger the release workflow."

## Start vanilla 1.21.4 server on port 25566 (for protocol comparison)
vanilla-start:
	docker compose up -d

## Stop vanilla server
vanilla-stop:
	docker compose down

## Show vanilla server logs
vanilla-logs:
	docker compose logs -f minecraft

## Attach to vanilla server console (Ctrl+P Ctrl+Q to detach)
vanilla-attach:
	docker attach basalt-minecraft-1
