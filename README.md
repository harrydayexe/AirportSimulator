# Airport Simulator

A deterministic, replicated airport simulator in Rust, running on a simulated network
inside the browser. See [`design/design-proposal.md`](design/design-proposal.md) for the plan
and [`AGENTS.md`](AGENTS.md) for layout and commands.

```sh
just setup   # one-off: wasm target, wasm-bindgen CLI, web deps
just dev     # http://localhost:3000
just ci      # check + test + build
```

Phase 1 is in place: one airport as a pure state machine (`crates/airport`), a traffic
generator, a command-line runner and a web page that draws a timeline for each runway.

```sh
just run demo --seed 7          # run the demonstration and print what happened
just run gen --seed 7 > s.json  # the same scenario as JSON: an airport and its commands
just run run s.json --json      # run any scenario file
```

Requires Rust (via rustup), [Bun](https://bun.sh) and [just](https://just.systems).

Licensed under the [MIT License](LICENSE).
