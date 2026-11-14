# AGENTS.md

Airport Simulator is a deterministic, replicated airport simulator. Everything runs in one
Rust program (the "host") on a simulated network, compiled to WebAssembly and driven by a
React web app. The full plan is in `design/design-proposal.md`.

## Structure

A Cargo workspace (`crates/`) plus a Bun/React app (`web/`). Each crate depends only on
those listed before it: `det` holds the deterministic tools everything shares (the seeded
`Rng`, the fingerprint, identifier types such as `NodeId`, `Term` and `Index`, and the
values a Raft node exchanges with the host, such as `Durable`, `Input` and `Effect`); `airport`
is the pure state machine and knows nothing of replicas,
networks or browsers; `raft` is the hand-written agreement logic; `host` runs replicas
behind the `Host` trait; `cli` (binary `airport-sim`) and `wasm` are thin
wrappers for the command line and the browser; `server` (binary `airport-server`) is one
real replica. `just wasm` compiles the
`wasm` crate and writes JS bindings to `web/src/wasm/` (generated, not committed), which
`web/src/sim.ts` loads.

## Simulated and production

The `Node` trait (`raft`) and the `Host` trait (`host`) each have two families of
implementation, and neither family may depend on the other:

- **Simulated** (`BrowserNode`, `ChildHost`, `BrowserHost`): every replica in one program
  on a fake network, disk and clock, for the browser and for seeded runs. Written by hand
  as the learning part of the project; do not complete or rewrite it unless asked.
- **Production** (`ProdNode`, `ProdHost`): one replica per machine. `ProdNode` is the
  complete Raft logic, still pure. `ProdHost` runs one `ProdNode` and writes its durable
  state to a `Storage` before any effect of a step is reported. Both are deterministic and
  are tested under simulation (`crates/raft/tests/prod_node.rs`,
  `RAFT_SEEDS=0..2000 cargo test --release -p raft --test prod_node` for more seeds).

`crates/server` is the only crate that touches the real world: it reads the clock, owns
the sockets (`transport.rs` between replicas, `http.rs` for clients) and the files
(`storage.rs`), and drives a `ProdHost` from one thread (`server.rs`). It has its own
`clippy.toml` with a shorter ban list for that reason; keep logic out of it.
`crates/server/tests/cluster.rs` runs three real processes and requires the same outputs
and fingerprint as `airport::scenario::run`.

Deployment lives in `Dockerfile`, `docker-compose.yml` (three replicas on one machine) and
`deploy/helm/airport-simulator` (a StatefulSet, one replica per node). Pushing a tag
`vX.Y.Z` runs `.github/workflows/release.yml`: binaries and the chart on the GitHub
release, the image and chart on ghcr.io.

## The airport crate

`Airport::apply(&Command) -> Vec<Output>` is the whole interface; time only moves through
`Command::AdvanceTime`. Every aviation number lives in `data.rs`, not in the logic. Tests in
`crates/airport/tests/rules.rs` show their hand-worked arithmetic; `tests/invariants.rs` feeds
random commands to random airports and checks the rules of design section 8
(`AIRPORT_SEEDS=0..5000 cargo test --release -p airport --test invariants` for more seeds).
`web/src/sim.test.ts` runs the same scenario natively and as WebAssembly and requires identical
output, so anything added to `Airport` must derive `Hash` to stay in the fingerprint.

## Invariants

`design/invariants.md` lists the assertions and the IDs (A7, N20, ...) the code cites. Anything a
`Command`, scenario file or CLI argument can cause is rejected, never asserted: a panic in
`Airport::apply` would stop every replica at the same log position. O(1) checks use `assert!`
(not `debug_assert!`, so native and browser builds stop at the same step), and release builds keep
overflow checks on. Whole-state scans (`Airport::check_invariants`, `BrowserNode::check_invariants`)
run after every `apply`/`step` in debug builds and tests, or in release with the `paranoid` feature.

## Determinism rules

Simulation code must be deterministic. `clippy.toml` and `[workspace.lints]` in `Cargo.toml`
ban the real clock, `HashMap`/`HashSet`, threads, locks and float arithmetic; use `BTreeMap`,
integer milliseconds, and avoid `usize` in saved state. Don't silence these lints; extend the
ban list in `clippy.toml` instead. Rust is pinned in `rust-toolchain.toml` (and as `RUST_VERSION` in the `Dockerfile`).

`missing_docs` is on and clippy's `pedantic` group warns, so every public item, field and
variant needs a doc comment, and `just check` (which denies warnings) fails without one.

## Commands

| Command | What it does |
|---|---|
| `just setup` | Install the wasm target, the pinned `wasm-bindgen` CLI and web deps |
| `just build` | Build the Rust workspace, the wasm bundle and the web app |
| `just test` | All tests (`just test-rust`, `just test-web`) |
| `just check` | `rustfmt --check`, clippy (native + wasm32), TypeScript typecheck, `check-ban` |
| `just check-ban` | Clippy on `tests/ban-canary/`, which must report every banned item |
| `just ci` | Everything CI runs: `check`, `test`, `web-build` |
| `just dev` | Rebuild wasm, then serve the web app at http://localhost:3000 |
| `just wasm` | Rebuild only the wasm bindings (rerun after Rust changes) |
| `just run ARGS` | Run the CLI, e.g. `just run demo --seed 7` (`just run` alone prints usage) |
| `just server ARGS` | Run one production replica (`just server --help` for options) |
| `just up` | Build the image and run three replicas with Docker Compose |
| `just image` / `just check-chart` | Build the container image / lint and render the Helm chart |
| `just fmt` / `just clean` | Format Rust / remove build outputs |

Run `just --list` for everything, grouped by setup, build, run, test and check.

Bump `wasm-bindgen` in three places together: `Cargo.toml`, `justfile`, `.github/workflows/ci.yml`.

## Commits

Always write commit messages in [Conventional Commits](https://www.conventionalcommits.org/)
style (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`, optional scope such as
`feat(raft):`). Versions are derived from them, so semver depends on getting the type right:
`fix` is a patch, `feat` is a minor, and a `!` after the type (or a `BREAKING CHANGE:` footer)
is a major.
