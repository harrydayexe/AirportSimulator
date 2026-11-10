# Airport Simulator: everyday commands. Run `just` to list them.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Must match the `wasm-bindgen` version pinned in Cargo.toml.
wasm_bindgen_version := "0.2.130"
wasm_target := "wasm32-unknown-unknown"
wasm_out := "web/src/wasm"

# List available commands
default:
    @just --list

# Install the tools this repo needs (wasm target, wasm-bindgen CLI, web deps)
setup:
    rustup target add {{wasm_target}}
    cargo install wasm-bindgen-cli --version {{wasm_bindgen_version}} --locked
    cd web && bun install

# Build everything: Rust workspace, WebAssembly and the web app
build: build-rust web-build

# Build the native Rust workspace
build-rust:
    cargo build --workspace

# Compile the wasm crate and generate JS bindings into web/src/wasm
wasm:
    cargo build -p airport-sim-wasm --target {{wasm_target}} --release
    wasm-bindgen --target web --out-dir {{wasm_out}} \
        target/{{wasm_target}}/release/airport_sim_wasm.wasm

# Production build of the web app into web/dist
web-build: wasm
    cd web && bun run build

# Run the web app dev server on http://localhost:3000
dev: wasm
    cd web && bun run dev

# Run the CLI, e.g. `just run demo --seed 7`
run *args:
    cargo run -p airport-sim -- {{args}}

# Run all tests
test: test-rust test-web

# Run Rust tests
test-rust:
    cargo test --workspace

# Run web tests (exercises the real wasm build)
test-web: wasm
    cd web && bun test

# Formatting, lints (incl. the determinism ban) and type checks
check: wasm
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p airport-sim-wasm --target {{wasm_target}} -- -D warnings
    cd web && bun run typecheck

# Format Rust code
fmt:
    cargo fmt --all

# Run what CI runs
ci: check test web-build

# Remove build outputs
clean:
    cargo clean
    rm -rf {{wasm_out}} web/dist
