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

## Running it for real

The simulator is for watching and testing. `airport-server` is the same idea deployed:
one replica per machine, three machines, real sockets and disks. Replicas agree through
Raft (`ProdNode`), and each writes its log to disk before it acknowledges anything.

```sh
just up                               # three replicas on this machine, via Docker Compose
curl localhost:8081/status            # role, term, positions, state fingerprint
curl -X POST localhost:8081/commands \
  -d '{"client":1,"seq":1,"command":{"type":"advance_time","now_ms":1000}}'
```

A client numbers its commands 1, 2, 3, ... (`seq`) under its own `client` number and
resends with the same `seq` until it gets `200`; a command is applied exactly once however
often it is sent. A replica that is not the leader answers `307` with the leader's number
and address. `GET /snapshot` shows a replica's airport; `GET /healthz` and `GET /readyz`
are for probes.

Without containers, start one process per machine with the same `--peers`:

```sh
airport-server --id 1 --peers 1=host-a:7000,2=host-b:7000,3=host-c:7000 --data-dir /var/lib/airport
```

On Kubernetes, the chart runs a StatefulSet with one replica per node:

```sh
helm install airport oci://ghcr.io/harrydayexe/charts/airport-simulator
```

Binaries for Linux, macOS and Windows, the container image
(`ghcr.io/harrydayexe/airportsimulator`) and the chart are published for every `v*` tag.
The peer port (7000) trusts anything that speaks its protocol: keep it on a private network.

Not there yet: the log is never trimmed (a restarted replica replays all of it), members
cannot be added or removed while the group runs, and clients are not authenticated.

Requires Rust (via rustup), [Bun](https://bun.sh) and [just](https://just.systems).

Licensed under the [MIT License](LICENSE).
