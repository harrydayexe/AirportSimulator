# AGENTS.md

Airport Simulator is a deterministic, replicated airport simulator. It is built as a set of
nodes that could each run on their own machine, and run in practice by one Rust program (the
"host") on a simulated network, natively for seeded tests and as WebAssembly for the React
demo. The full plan is in `design/design-proposal.md`; Phase 2 is in `design/phase-2-spec.md`.

## Vocabulary

- **Node**: one participant in the protocol, either a replica or a controller. It has a
  `NodeId`, takes inputs through `step` and returns effects, and never touches a clock,
  network, disk or another node directly.
- **Replica**: a node holding one copy of an airport: the `Airport`, a Raft member and its log.
- **Controller**: a node that owns the nodes below it (it knows their ids and speaks for them;
  it never holds them in memory). A `LocalController` owns the three replicas of one airport.
  An `AreaController` (Phase 4) owns several `LocalController`s.
- **Raft member** (`raft::Member`): the agreement logic inside a replica. Not a node.
- **Host**: the simulated runtime. Flat: one map of nodes, one queue, one clock.

There is one implementation of every node; only the runtime differs between the demo and a
real deployment. Never add a browser-only or simulator-only variant of anything in `raft` or
`node`, and never make those crates (or `det`, `airport`) depend on `host`.

## Structure

A Cargo workspace (`crates/`) plus a Bun/React app (`web/`). Each crate depends only on
those listed before it: `det` holds the deterministic tools everything shares (the seeded
`Rng`, the fingerprint, identifier types such as `NodeId`, `Term` and `Index`, and the
values a Raft member exchanges with its replica, such as `Durable`, `Input` and `Effect`); `airport`
is the pure state machine and knows nothing of replicas,
networks or browsers; `raft` is the hand-written agreement logic (`Member`); `node` holds the
`Node` trait and, as Phase 2 lands, the replica and the `LocalController`; `host` runs every
node with a fake network, disk and clock; `cli` (binary `airport-sim`) and `wasm` are thin
wrappers around `host` for bulk seeded runs and for the browser. `just wasm` compiles the
`wasm` crate and writes JS bindings to `web/src/wasm/` (generated, not committed), which
`web/src/sim.ts` loads.

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
overflow checks on. Whole-state scans (`Airport::check_invariants`, `Member::check_invariants`)
run after every `apply`/`step` in debug builds and tests, or in release with the `paranoid` feature.

## Determinism rules

Simulation code must be deterministic. `clippy.toml` and `[workspace.lints]` in `Cargo.toml`
ban the real clock, `HashMap`/`HashSet`, threads, locks and float arithmetic; use `BTreeMap`,
integer milliseconds, and avoid `usize` in saved state. Don't silence these lints; extend the
ban list in `clippy.toml` instead. Rust is pinned in `rust-toolchain.toml`.

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
| `just fmt` / `just clean` | Format Rust / remove build outputs |

Run `just --list` for everything, grouped by setup, build, run, test and check.

Bump `wasm-bindgen` in three places together: `Cargo.toml`, `justfile`, `.github/workflows/ci.yml`.

## Commits

Always write commit messages in [Conventional Commits](https://www.conventionalcommits.org/)
style (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`, optional scope such as
`feat(raft):`). Versions are derived from them, so semver depends on getting the type right:
`fix` is a patch, `feat` is a minor, and a `!` after the type (or a `BREAKING CHANGE:` footer)
is a major.
