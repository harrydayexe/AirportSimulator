# Airport Simulator

A deterministic, replicated airport simulator in Rust, running on a simulated network
inside the browser. See [`design/design-proposal.md`](design/design-proposal.md) for the plan
and [`AGENTS.md`](AGENTS.md) for layout and commands.

```sh
just setup   # one-off: wasm target, wasm-bindgen CLI, web deps
just dev     # http://localhost:3000
just ci      # check + test + build
```

Requires Rust (via rustup), [Bun](https://bun.sh) and [just](https://just.systems).

Licensed under the [MIT License](LICENSE).
