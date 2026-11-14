# Airport Simulator: everyday commands. Run `just` to list them.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Must match the `wasm-bindgen` version pinned in Cargo.toml and .github/workflows/ci.yml.
wasm_bindgen_version := "0.2.130"
wasm_target := "wasm32-unknown-unknown"
wasm_out := "web/src/wasm"

# List available commands
[private]
default:
    @just --list

# Install the tools this repo needs (wasm target, wasm-bindgen CLI, web deps)
[group("setup")]
setup:
    rustup target add {{ wasm_target }}
    cargo install wasm-bindgen-cli --version {{ wasm_bindgen_version }} --locked
    cd web && bun install

# Build everything: Rust workspace, WebAssembly and the web app
[group("build")]
build: build-rust web-build

# Build the native Rust workspace
[group("build")]
build-rust:
    cargo build --workspace

# Compile the wasm crate and generate JS bindings into web/src/wasm
[group("build")]
wasm:
    cargo build -p airport-sim-wasm --target {{ wasm_target }} --release
    wasm-bindgen --target web --out-dir {{ wasm_out }} \
        target/{{ wasm_target }}/release/airport_sim_wasm.wasm

# Production build of the web app into web/dist
[group("build")]
web-build: wasm
    cd web && bun run build

# Remove build outputs
[group("build")]
clean:
    cargo clean
    rm -rf {{ wasm_out }} web/dist

# Run the web app dev server on http://localhost:3000
[group("run")]
dev: wasm
    cd web && bun run dev

# Run the CLI, e.g. `just run demo --seed 7`
[group("run")]
[positional-arguments]
run *args:
    cargo run -p airport-sim -- "$@"

# Run one replica of the production server, e.g. `just server --id 1 --peers 1=127.0.0.1:7000`
[group("run")]
[positional-arguments]
server *args:
    cargo run --release -p airport-server -- "$@"

# Build the production container image
[group("build")]
image:
    docker build --tag ghcr.io/harrydayexe/airportsimulator:latest .

# Run three replicas with Docker Compose (clients on localhost:8081 to 8083)
[group("run")]
up:
    docker compose up --build

# Lint and render the Helm chart
[group("check")]
check-chart:
    helm lint --strict deploy/helm/airport-simulator
    helm template airport deploy/helm/airport-simulator > /dev/null

# Run all tests
[group("test")]
test: test-rust test-web

# Run Rust tests
[group("test")]
test-rust:
    cargo test --workspace

# Run web tests (exercises the real wasm build)
[group("test")]
test-web: wasm
    cd web && bun test

# Formatting, lints (incl. the determinism ban) and type checks
[group("check")]
check: wasm check-ban
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p airport-sim-wasm --target {{ wasm_target }} -- -D warnings
    cd web && bun run typecheck

# Check the determinism ban still catches one of each banned item
[group("check")]
[script("bash")]
check-ban:
    set -euo pipefail
    output=$(CLIPPY_CONF_DIR="$PWD" cargo clippy --quiet \
        --manifest-path tests/ban-canary/Cargo.toml --target-dir target/ban-canary \
        -- -D clippy::disallowed_methods -D clippy::disallowed_types -D clippy::float_arithmetic \
        2>&1 || true)
    status=0
    for expected in \
        'disallowed method `std::time::Instant::now`' \
        'disallowed type `std::collections::HashMap`' \
        'disallowed method `std::thread::spawn`' \
        'disallowed type `std::sync::Mutex`' \
        'floating-point arithmetic detected'; do
        if ! grep -qF "$expected" <<<"$output"; then
            echo "the ban no longer reports: $expected" >&2
            status=1
        fi
    done
    exit "$status"

# Format Rust code
[group("check")]
fmt:
    cargo fmt --all

# Run what CI runs
[group("check")]
ci: check test web-build
