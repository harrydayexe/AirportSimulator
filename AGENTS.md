# AGENTS.md

Airport Simulator is a deterministic, replicated airport simulator. Everything runs in one
Rust program (the "host") on a simulated network, compiled to WebAssembly and driven by a
React web app. The full plan is in `docs/design-proposal.md`.

## Structure

A Cargo workspace (`crates/`) plus a Bun/React app (`web/`). Each crate depends only on
those listed before it: `airport` is the pure state machine and knows nothing of replicas,
networks or browsers; `raft` is the hand-written agreement logic; `host` runs every replica
with a fake network, disk and clock; `cli` (binary `airport-sim`) and `wasm` are thin
wrappers around `host` for bulk seeded runs and for the browser. `just wasm` compiles the
`wasm` crate and writes JS bindings to `web/src/wasm/` (generated, not committed), which
`web/src/sim.ts` loads.

## Determinism rules

Simulation code must be deterministic. `clippy.toml` and `[workspace.lints]` in `Cargo.toml`
ban the real clock, `HashMap`/`HashSet`, threads, locks and float arithmetic; use `BTreeMap`,
integer milliseconds, and avoid `usize` in saved state. Don't silence these lints; extend the
ban list in `clippy.toml` instead. Rust is pinned in `rust-toolchain.toml`.

## Commands

| Command | What it does |
|---|---|
| `just setup` | Install the wasm target, the pinned `wasm-bindgen` CLI and web deps |
| `just build` | Build the Rust workspace, the wasm bundle and the web app |
| `just test` | All tests (`just test-rust`, `just test-web`) |
| `just check` | `rustfmt --check`, clippy (native + wasm32), TypeScript typecheck |
| `just ci` | Everything CI runs: `check`, `test`, `web-build` |
| `just dev` | Rebuild wasm, then serve the web app at http://localhost:3000 |
| `just wasm` | Rebuild only the wasm bindings (rerun after Rust changes) |
| `just run [NM KT]` | Run the CLI |
| `just fmt` / `just clean` | Format Rust / remove build outputs |

Bump `wasm-bindgen` in three places together: `Cargo.toml`, `justfile`, `.github/workflows/ci.yml`.
