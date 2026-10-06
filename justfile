# Run `just` to see available recipes.

set positional-arguments := true

# List recipes
default:
    @just --list

# Build debug binary
build:
    cargo build

# Build optimized release binary
release:
    cargo build --release

# Run the CLI, e.g. `just run rust-lang rust octocat`
run *args:
    cargo run -- "$@"

# Format sources
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt --check

# Lint with clippy, treating warnings as errors
lint:
    cargo clippy --all-targets -- -D warnings

# Run tests
test:
    cargo test

# Mutation testing (requires `cargo install cargo-mutants`)
mutants:
    cargo mutants

# Format check, lint, and test
check: fmt-check lint test

# Install the binary to ~/.cargo/bin
install:
    cargo install --path . --locked

# Show what a release would build (requires `brew install cargo-dist`)
dist-plan:
    dist plan

# Build release artifacts for this machine into target/distrib
dist-build:
    dist build

# Regenerate .github/workflows/release.yml after editing dist-workspace.toml
dist-generate:
    dist generate

# Tag and push a release, e.g. `just release-tag 0.2.0` (bump + commit Cargo.toml version first)
release-tag version:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo_version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
    if [[ "$cargo_version" != "{{version}}" ]]; then
        echo "Cargo.toml is at $cargo_version but you asked to tag {{version}}" >&2
        exit 1
    fi
    if [[ -n "$(git status --porcelain)" ]]; then
        echo "working tree is dirty; commit or stash first" >&2
        exit 1
    fi
    git tag -a "v{{version}}" -m "v{{version}}"
    git push origin "v{{version}}"

# Remove build artifacts
clean:
    cargo clean
