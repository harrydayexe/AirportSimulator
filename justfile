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
check: wasm
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p airport-sim-wasm --target {{ wasm_target }} -- -D warnings
    cd web && bun run typecheck

# Format Rust code
[group("check")]
fmt:
    cargo fmt --all

# Run what CI runs
[group("check")]
ci: check test web-build
