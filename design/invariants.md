# Airport Simulator: invariants to assert

Reviewed at commit `dead33d` plus the uncommitted `host/src/{hash,seeds}.rs`, against the design proposal (Phases 0 and 1) and the Phase 2 spec in the vault (the 1,210-line version, which is newer than `design/phase-2-spec.md` in the repo).

I built a scratch copy with Rust 1.97.0 and ran it. All 78 existing Rust tests pass. Where this document says **fires today**, I confirmed the behaviour with a throwaway probe test on that copy; nothing in your repo was touched.

## 1. Ground rules

**What is an invariant and what is input.** The airport receives its commands from a replicated log. If `Airport::apply` panics on a command, all three replicas panic at the same log position and the group can never recover. So:

- A state the code itself should have made impossible gets an **assertion**.
- Anything a `Command`, a scenario JSON file or a CLI argument can cause gets a **rejection** (`Output::Rejected`, `ConfigError`, `Err(String)`), never an assertion. Section 2 lists the inputs that reach a panic today and need a rejection first.
- Messages between Raft nodes sit in between. Every node runs your code, so a malformed message is a bug in the sender, and asserting on receipt is right for the simulator. If Rule 3 is ever used to put this on a real network, those become validation.

**Which macro.**

| Form | Use for | Why |
|---|---|---|
| `const _: () = assert!(..)` | Facts about the data tables and constants | Fails the build, not the run. |
| `assert!` | Any O(1) check | Always on, so the native build and the release WebAssembly build stop at the same step. `debug_assert!` would make a debug run and a browser run disagree. |
| `fn check_invariants(&self)` behind `#[cfg(any(test, debug_assertions, feature = "paranoid"))]` | Whole-state scans (O(n)) | Called at the end of `Airport::apply` and `Node::step`, and from `tests/invariants.rs` after every command. The bulk seeded runs of Phase 2 turn the feature on. |
| `expect("…")` / `unreachable!("…")` | Replacing a silent `else { return }` on an impossible branch | Several of these exist today and hide broken state. Listed per function below. |

**Two workspace-level changes that do most of the work.**

- **G1.** Add `overflow-checks = true` to `[profile.release]`. Today an integer overflow panics in debug and silently wraps in release, and the browser runs release. That is a determinism break between the two builds the project promises are identical. With this one line every `+` on `Ms`, `Seq`, `Term`, `Index` and the stats counters becomes an assertion in both.
- **G2.** Remove `#[non_exhaustive]` from the types in `det::objects` and `host::seeds::Stream`. They are only matched inside this workspace, and the attribute forces a `_ => todo!()` arm in `raft` (there are two today). Without it the compiler proves exhaustiveness and no runtime check is needed.

**Assertions must not change the run.** No assertion may draw from an `Rng`, allocate into saved state, or reorder anything.

Column **Kind**: PRE = precondition, POST = postcondition, INV = holds whenever the struct is at rest, CONST = compile time, LOOP = inside a loop.

## 2. Inputs that reach a panic today (reject these, do not assert)

| ID | Input | What happens now | Suggested fix |
|---|---|---|---|
| X1 | `RequestLanding { earliest_ms: u64::MAX }` then any `AdvanceTime`, on an airport with a hold limit | **Fires today:** `attempt to add with overflow` in `divert_overdue` (`key.ready_ms + hold`). In release it wraps and the flight is diverted at a nonsense time. | A `MAX_TIME_MS` constant (say 100 simulated years). `request` and `advance_time` reject anything above it with a new `RejectReason::InvalidTime`. Then INV A12 below can assert `now_ms <= MAX_TIME_MS`. |
| X2 | `AdvanceTime { now_ms: u64::MAX }` with a flight ready | **Fires today:** overflow in `perform` (`at_ms + occupancy`). | Same cap as X1. |
| X3 | Scenario JSON with huge `exit_extra_ms`, `mayday_inspection_ms`, `max_arrival_hold_ms` or `final_approach_nm` | Unbounded, so the same overflows are reachable from a config file. | Bound each in `AirportConfig::validate` with a new `ConfigError::OutOfRange(field)`. |
| X4 | Runway end name that is empty or contains `/` | `is_named` and `runway_index` become ambiguous (`"09/27"` could be an end or a full name). | Reject in `validate`. |
| X5 | `policy::curfew` with `start_of_day_ms` more than a day after `end_of_day_ms` | **Fires today:** `attempt to subtract with overflow` (start 31 h, end 6 h). Start 30 h gives a zero-length curfew. | These arguments come from code, not from a user, so here an assertion is right: see P1. |

## 3. Phase 0: the build

These are invariants of the repository, enforced by tooling, not by `assert!`.

| ID | Invariant | Enforced by |
|---|---|---|
| Z1 | No real clock, `HashMap`/`HashSet`, threads, locks or float arithmetic in simulation crates. | `clippy.toml`, `[workspace.lints]`. Already in place. |
| Z2 | Debug and release agree on overflow. | G1 above. **Not in place.** |
| Z3 | `wasm-bindgen` version is the same in `Cargo.toml`, `justfile` and `ci.yml`. | Nothing today. See test case P0-2. |
| Z4 | `size_of::<usize>() <= 8`, so `Fnv1a::write_usize` loses nothing. | `const _: () = assert!(size_of::<usize>() <= 8);` in `fingerprint.rs`. |
| Z5 | Gaps in the ban list worth closing: `std::env::var`, `std::sync::atomic`, `std::sync::mpsc`, `std::collections::hash_map::RandomState`, `std::thread::current`. | Extend `clippy.toml`. |

## 4. `det` crate

### `rng.rs`

| ID | Function | Kind | Invariant |
|---|---|---|---|
| R1 | `Rng::new` | POST | `s != [0; 4]`. The all-zero state is the one state xoshiro can never leave. SplitMix64 cannot produce it, so this is truly impossible. |
| R2 | `Rng::below(n)` | POST | `n == 0 \|\| result < n`. |
| R3 | `Rng::below(0)` | — | Decide: it returns 0 today and a test pins that. Every current caller that passes 0 does so by accident (`traffic::generate` with an empty mix or zero duration). I would keep the behaviour and assert at the callers instead (T1, T2). |

### `ids.rs`

| ID | Function | Kind | Invariant |
|---|---|---|---|
| I1 | `Ms + Ms` | POST | No overflow: `Ms(self.0.checked_add(rhs.0).expect("simulated time overflow"))`. G1 covers it too, but this gives a message. |
| I2 | `Index` | INV | Positions start at 1. Any helper that turns an `Index` into a slot in `log` asserts `1 <= index.0 && index.0 <= log.len()`. `Index(0)` is only legal as "before the first entry" (`prev_index`, `matched`, `commit`). |
| I3 | `Term` | INV | `Term(0)` is only the term of a node that has never seen an election. No entry and no message carries term 0. |

### `config.rs` (`det::Config`)

| ID | Function | Kind | Invariant |
|---|---|---|---|
| C1 | `Node::new` (or a new `Config::validate`) | PRE | `heartbeat_interval > 0`, `base_election_timeout > 0`, and `heartbeat_interval < base_election_timeout`. Otherwise followers time out between heartbeats and no leader survives. |
| C2 | same | PRE | `2 * base_election_timeout` does not overflow. |

### `objects.rs`

Written for the field types the spec's section 11.11 asks for (`prev_term: Term`, `entries: Vec<Entry<C>>`, `Index` not `usize`).

| ID | Type / function | Kind | Invariant |
|---|---|---|---|
| O1 | `Durable` (checked in `Node::new`) | PRE | Every `entry.term >= 1`, entry terms never decrease along the log, and the last entry's term `<= durable.term`. |
| O2 | `Durable` | PRE | `term == 0` implies `voted_for == None` and an empty log. |
| O3 | `Durable` | PRE | `voted_for`, if set, is this node or one of its peers. |
| O4 | `Message` (checked on receipt) | PRE | `term >= 1`. |
| O5 | `AppendEntries` | PRE | `prev_index == 0` exactly when `prev_term == 0`. Entry terms never decrease, the first is `>= prev_term`, the last is `<= message.term`. `leader_commit <= prev_index + entries.len()` is **not** required (the leader may be further ahead), which is why N19 exists. |
| O6 | `AppendReply` (leader side) | PRE | For a same-term successful reply, `last_match_index <= log.len()`. A follower cannot hold more of this term's log than the leader has. |
| O7 | `Effect::Send` (checked where effects are built) | POST | `to != self.id`, `to` is a peer, and `message.term == self.term` at the moment of sending. |
| O8 | `Effect::NotLeader` | POST | `hint != Some(self.id)`, and it is only emitted in answer to `Propose`. |
| O9 | `Effect::Committed` | POST | Within one `step`, indices are consecutive and start at the old commit position plus one. |

## 5. `raft` crate: `BrowserNode`

### Struct invariants (`check_invariants`, run at the end of `new` and of every `step`)

| ID | Invariant | Note |
|---|---|---|
| N1 | `id` is not in `peers`, and `peers` has no repeats. | `cluster_size` is wrong otherwise. The existing `assert_ne!(cs, 0)` in `majority` can never fire, since `cs` is `len + 1`; N1 is the check that actually protects it. |
| N2 | `2 * majority() > cluster_size()` and `2 * (majority() - 1) <= cluster_size()`. | True for the formula today (verified for 1 to 6 nodes: 1, 2, 2, 3, 3, 4). |
| N3 | Leader: `heartbeat_deadline.is_some()` (unless it has no peers) and `election_deadline.is_none()`. Follower or candidate: `election_deadline.is_some()` and `heartbeat_deadline.is_none()`. | **Fires today:** `new` leaves both `None`, so a new node can never start an election. Spec 10.7 sets the election deadline in `new`. |
| N4 | `next_wake()` equals the deadline for the current role. | **Fires today:** the `next_wake` field is set to `None` in `new` and never written again, so the host would never schedule a `Wake`. Spec 10.6 makes it a pure read of the deadlines; deleting the field removes the invariant. |
| N5 | Candidate: `voted_for == Some(id)`, `votes` contains `id`, and `votes` is a subset of `{id} ∪ peers`. | **Fires today** on the subset part: a `VoteReply` from `NodeId(99)` is counted. |
| N6 | Leader: `voted_for == Some(id)`. | A leader won this term having voted for itself, and any term change demotes it first. |
| N7 | Not a candidate: `votes` is empty. | Clear it on every role change. |
| N8 | Log shape: O1 holds for `log` and `term` at all times. | |
| N9 | `commit <= log.len()`. (When `commit` is added.) | |
| N10 | Leader, for each peer: `1 <= next[p] <= log.len() + 1`, `matched[p] <= log.len()`, `matched[p] < next[p]`. (When added.) | |
| N11 | `leader_hint != Some(id)` unless this node is the leader. | |

### Step-level invariants (compare the state at the start and end of one `step`)

| ID | Invariant | Raft rule it protects |
|---|---|---|
| N12 | `now >= self.now`, then store it. | Time never goes backwards for a node. Today `self.now` is set in `new` and never updated. |
| N13 | `term` never decreases. | Terms only rise. |
| N14 | If `term` did not change and `voted_for` was `Some(x)`, it is still `Some(x)`. | One vote per term. |
| N15 | If the node was leader at the start and is leader at the end in the same term, its old log is a prefix of its new log. Cheap form: `log.len()` did not shrink and the old last entry's term is unchanged. | Leaders only append. |
| N16 | `commit` never decreases. | Commit only rises. |
| N17 | A role change to leader happens only from candidate; to candidate only from follower or candidate. | The state diagram in spec 10.1. |

### Per function

| ID | Function | Kind | Invariant |
|---|---|---|---|
| N18 | `new` | POST | `role == Follower` whatever `durable` says, `votes` empty, `leader_hint == None`, plus C1, O1 to O3, N1. |
| N19 | `handle_message` | PRE | `from != self.id` and `from` is a peer. A message from a stranger or from itself is a host routing bug. |
| N20 | `handle_message`, at the dispatch `match` | INV | `message.term == self.term`. Higher terms were adopted and lower ones answered or dropped above it. **Fires today:** a stale *reply* is not returned early (the `return` is inside the `is_request` branch), so it falls through. Probe: a candidate in term 5 with one vote receives `VoteReply` from term 3, counts it, and reaches `become_leader`. |
| N21 | `handle_message`, higher term seen | POST | `voted_for == None`, `role == Follower`, `leader_hint == None`. **Fires today** on the hint: it is set to `Some(from)`, but the sender may be a candidate, so a later `NotLeader` points clients at a node that is not the leader. |
| N22 | `RequestVote`, on grant | POST | `voted_for == Some(from)`, exactly one `VoteReply` is sent to `from` carrying `self.term`, and the election deadline was reset. On refusal the deadline is unchanged. |
| N23 | `VoteReply` | PRE | Counted only if `role == Candidate` (and, by N20, the reply is from this term). |
| N24 | `AppendEntries`, same term | PRE | `role != Leader`. Two leaders in one term is a safety violation: panic with both ids and the term. |
| N25 | `AppendEntries`, on deleting a conflicting suffix starting at position `i` | PRE | `i > commit`. A committed entry is never removed. This is the single most valuable assertion in the crate. |
| N26 | `AppendEntries`, follower commit | POST | `commit == max(old_commit, min(leader_commit, prev_index + entries.len()))`, and N9. |
| N27 | `AppendReply`, on success | POST | `matched[from]` did not decrease, and `next[from] == matched[from] + 1`. |
| N28 | `start_election` | PRE / POST | PRE: `role != Leader`. POST: `term == old + 1`, `role == Candidate`, `voted_for == Some(id)`, `votes == {id}`, and `now + base <= election_deadline <= now + 2 * base`. |
| N29 | `become_leader` | PRE / POST | PRE: `role == Candidate` and `votes.len() >= majority()`. POST: the last log entry is `Noop` with `term == self.term`, `election_deadline == None`, every `matched[p] == 0`, and every `next[p]` is the position of that new `Noop`. |
| N30 | `advance_commit` | POST | For the new commit position `N`: `log[N].term == self.term`, and the number of nodes holding `N` (leader included) is `>= majority()`. |
| N31 | `append_entry` (leader `Propose`) | PRE / POST | PRE: `role == Leader`. POST: `log.len() == old + 1` and the new entry's term is `self.term`. |
| N32 | `reset_heartbeat_deadline` | PRE | `role == Leader`. |
| N33 | `send_append_to_all` | PRE | `role == Leader`. |
| N34 | `send_refusal` | PRE | `term == self.term` and the message being refused was a request. |
| N35 | `step`, `Input::Wake` | — | Must be harmless at any time. **Fires today:** the arm is `todo!()`. |
| N36 | `next_wake` | POST | Calling it twice gives the same answer and leaves `rng` untouched (it takes `&self`, so the compiler already guarantees this as long as `rng` is not put behind a `Cell`). |
| N37 | `into_durable` | POST | Returns exactly `term`, `voted_for`, `log`. Nothing volatile leaks in. |

## 6. `host` crate

### `clock.rs`

| ID | Function | Kind | Invariant |
|---|---|---|---|
| K1 | `Clock::advance_to` | PRE | `to >= self.now`. Present. Add a message with both times. |

### `queue.rs`

| ID | Function | Kind | Invariant |
|---|---|---|---|
| Q1 | `push` | PRE | The new key is not already present: `assert!(self.events.insert((at, s), event).is_none())`. This replaces the `max_seq` check. **Fires today:** the current assertion `s > self.max_seq()` panics on the very first `push`, because an empty queue reports a maximum of `Seq(0)` and the first `Seq` is also 0. Nothing can be queued at all. |
| Q2 | `push` | PRE | `at >= floor`, where `floor` is the time of the last `pop`. Nothing may be scheduled in the past. The `floor` field exists but is never written. |
| Q3 | `pop` | POST | The popped time is `>= floor`, then `floor` becomes that time. Popped keys are strictly increasing over the life of the queue. |
| Q4 | `pop` | POST | The popped key equals what `peek` returned. Present. |
| Q5 | `Seq::next` | POST | No overflow. |
| Q6 | struct | INV | `seqs.len() == events.len()`. Or delete `seqs`: it is never pruned on `pop`, so it grows for ever, and `next_seq` alone already guarantees uniqueness. |

### `seeds.rs`, `hash.rs`, `lib.rs`

| ID | Function | Kind | Invariant |
|---|---|---|---|
| S1 | `Seeds::rng` | CONST | The three stream tags (1, 2, 3) are distinct. Make them named constants and `const`-assert it. Independence of streams cannot be asserted cheaply; it is a test (H-S cases). |
| S2 | `Seeds::child` | POST | `child(i).root != self.root`. A child sharing its parent's root would replay the parent's network stream. |
| S3 | `mix` | — | Pure, no invariant. Note that the doc comment says "SplitMix64 finaliser" but all three multiplications use `GOLDEN`; SplitMix64 uses `0xbf58476d1ce4e5b9` and `0x94d049bb133111eb`. Either is deterministic. Pin whichever you keep (test H-M1). |
| L1 | `demo_scenario_json` | POST | Serialising a `Scenario` cannot fail, so `unwrap_or_default()` turning a failure into `""` hides an impossible state. Use `expect`. |

### Not written yet (from spec section 11, for when the code lands)

| ID | Function | Kind | Invariant |
|---|---|---|---|
| H1 | `Host::step(now)` | PRE | `now == self.next_event_at()` and `now >= last_now`. |
| H2 | `Host::inject(at, ..)` | PRE | `at >= last_now`, and `self.owns(event.target())`. |
| H3 | `ChildHost::new` | PRE | `here` is non-empty, has no repeats, and is a subset of `cluster`. |
| H4 | `ChildHost::step_node` | POST | Every `Send` becomes exactly one `Deliver` or one `Outbound`, never both, never neither. |
| H5 | `ChildHost::reschedule_wake` | POST | `scheduled_wake[id] == node.next_wake()` after the call, and any `Wake` pushed is at `>= now`. |
| H6 | `BrowserHost::add_child` | PRE | No node id of the child is already owned. After it, every node id is owned by exactly one host. |
| H7 | `BrowserHost::route` | POST | An `Outbound` is injected into exactly one owner, or passed up. |
| H8 | `BrowserHost::next_source` | POST | The chosen `(time, source)` is the minimum over all hosts, and `time >= clock.now()`. |
| H9 | Replica (D1) | PRE | On `Committed { index }`: `index == last_applied + 1`. The airport is only ever changed here. |
| H10 | Replica (D2) | PRE | `seq <= row + 1` for the client's row. A gap is a client bug: the spec says fail loudly. |
| H11 | Client (D3) | INV | At most one command outstanding, and `seq` never decreases. |
| H12 | Crash and restart (C3) | POST | The restarted node's `Durable` equals what the crashed node handed over, and `life` went up by exactly one. |

## 7. `airport` crate

### `data.rs` (all compile time)

| ID | Kind | Invariant |
|---|---|---|
| D1 | CONST | Every `AIRCRAFT_TYPES[i].approach_speed_kt > 0`. `approach_gap_ms` divides by it. |
| D2 | CONST | Every `runway_m > 0`, and `AIRCRAFT_TYPES.len() <= u16::MAX`. |
| D3 | CONST | `MIN_OCCUPANCY_MS` is the smallest entry of both occupancy tables. Today this is a unit test; `sealed` depends on it, so make it a build failure. |
| D4 | CONST | `SIN_E4[0] == 0`, `SIN_E4[90] == 10_000`, and the table never decreases. |
| D5 | CONST | In both wake tables each row never increases from left to right: a heavier follower never needs more room than a lighter one behind the same leader. |
| D6 | CONST | `PRIORITY_MAYDAY < PRIORITY_PAN_PAN < PRIORITY_NORMAL` (in `state.rs`). `divert_overdue` relies on normal being the largest. Also `(Emergency::PanPan as u8) < (Emergency::Mayday as u8)`: the "upgrade only" rule uses the derived order. |
| D7 | `aircraft(index)` PRE | `index < AIRCRAFT_TYPES.len()`, with a message. It already panics by indexing, but anonymously. |

Unique ICAO codes cannot be checked in a `const` without string comparison helpers; the existing `aircraft_table_is_consistent` test covers it.

### `separation.rs`, `wind.rs`

| ID | Function | Kind | Invariant |
|---|---|---|---|
| P-S1 | `approach_gap_ms` | PRE | Both approach speeds `> 0`. |
| P-S2 | `approach_gap_ms` | POST | `result >= ceil(sep * MS_PER_HOUR / vf)`: the slow-follower gap is never shorter than the plain gap. And `result > 0`. |
| W1 | `sin_e4(deg)` | PRE | `deg <= 90`. Today it clamps with `.min(90)`, which would turn a geometry bug into a wrong-but-plausible crosswind. |
| W2 | `components` | POST | `crosswind_e4 <= speed * 10_000` and `headwind_e4.unsigned_abs() <= speed * 10_000`. |

### `config.rs`

| ID | Function | Kind | Invariant |
|---|---|---|---|
| V1 | `validate` | POST | `masks.len() == runways.len()`. No mask has its own bit set, or any bit at or above `runways.len()`. The masks are symmetric: bit `b` of `masks[a]` equals bit `a` of `masks[b]`. |

### `state.rs`: whole-state invariants (`Airport::check_invariants`)

| ID | Invariant |
|---|---|
| A1 | `runways.len() == config.runways.len() == crossing.len()`, between 1 and `MAX_RUNWAYS`. |
| A2 | Queue and index agree: for every `(key, flight)` in `queue`, `flights[flight.id] == Queued(key)`; and the number of `Queued` entries in `flights` equals `queue.len()`. |
| A3 | For every queued flight: `key.priority == priority(flight.emergency)`, `key.ready_ms == flight.ready_ms`, `key.seq < next_seq`, and no two keys share a `seq`. |
| A4 | `stats.accepted == flights.len()`, and the counts of `Landed`, `Departed`, `Diverted` and `Queued` statuses equal `stats.landed`, `stats.departed`, `stats.diverted` and `queue.len()`. So `accepted == landed + departed + diverted + queue.len()`. |
| A5 | Every queued flight's `aircraft` index is in the table, and some runway is long enough for it. Otherwise it would wait for ever. |
| A6 | `wind.direction_deg < 360`. |
| A7 | The active set is legal: every active runway has no closures and a crosswind within its limit; no two active runways cross; and if any runway is a candidate, the active set is non-empty and contains one of the longest candidates. |
| A8 | `last_arrival.at_ms <= now_ms` and `last_departure.at_ms <= now_ms` on every runway. Movements are never performed in the future. |
| A9 | Every flight with status `Diverted` was an ordinary arrival when it left the queue. Emergencies and departures never divert. |
| A10 | After `advance_time` returns: no ordinary arrival is still queued with `ready_ms + hold <= now_ms`. |
| A11 | Across one `apply`: `now_ms` never decreases, and changes only for `AdvanceTime`. `next_seq`, every stats counter and every `inspection_until_ms` never decrease. |
| A12 | `now_ms <= MAX_TIME_MS` (after X1). |

### `state.rs`: per function

| ID | Function | Kind | Invariant |
|---|---|---|---|
| A13 | `apply` | POST | Every output's `at_ms <= now_ms`. For `Landed` and `Departed`: `vacated_ms > at_ms`, and `at_ms >=` the clock time before this command. |
| A14 | `reject` | POST | Exactly one output is pushed and only `stats.rejected` changed. (Whole-state comparison is a test, AP-R1, not an assertion.) |
| A15 | `request`, accept path | POST | `queue.len()`, `flights.len()` and `stats.accepted` each rose by one; `flight.ready_ms >= now_ms`. |
| A16 | `enqueue` | POST | `queue.insert(key, flight)` returned `None`. A key collision would silently drop a flight. |
| A17 | `declare_emergency` | — | `queue.remove(&key)` returning `None` contradicts A2. Replace `else { return; }` with `expect`. |
| A18 | `declare_emergency` | POST | `queue.len()` unchanged, and the flight's emergency did not go down. |
| A19 | `set_closure` | POST | After `reconfigure`: a runway with any closure is not active. |
| A20 | `reconfigure` | PRE | `count <= MAX_RUNWAYS`, so `1u16 << count` cannot overflow. |
| A21 | `reconfigure` | POST | A7. Also `best_set & !candidates == 0`, and `candidates != 0` implies `best_set != 0`. |
| A22 | `gap` | POST | `result >= MIN_OCCUPANCY_MS`. `sealed` is only sound if no gap can be shorter. |
| A23 | `runway_time` | POST | `Some(t)` implies `t >= flight.ready_ms`, `t >= floor_ms`, `t >= busy_until(r)`, and `t >= busy_until(c)` for every crossing runway `c`. This is "one at a time" and "layout respected", checked at the point of decision. |
| A24 | `schedule` | PRE | `floor_ms <= now_ms`. |
| A25 | `schedule` | LOOP | `open` never underflows: `open = open.checked_sub(1).expect(..)`. A runway already counted as sealed never has a movement performed on it later in the same walk (keep a `sealed: Vec<bool>` and assert before `perform`). |
| A26 | `schedule` | LOOP | A claim is only recorded with `claim.at_ms > now_ms`. |
| A27 | `schedule`, just before `perform` | PRE | `floor_ms <= t <= now_ms`; `ground_stops` is empty; runway `r` is active and long enough; and if `claims[r]` is `Some(claim)`, then `t + gap_behind(r, flight, claim) <= claim.at_ms`. The last clause is "emergencies first": nobody is performed who would delay a flight ahead of them in the queue. |
| A28 | `schedule` | — | `queue.remove(&key)` returning `None` is impossible (the key was just read from the queue). Replace `else { break }` with `expect`. |
| A29 | `perform` | PRE | `at_ms >= flight.ready_ms`. The subtraction for `delay_ms` underflows otherwise (panic in debug, wrap in release). |
| A30 | `perform` | PRE | `at_ms >= inspection_until_ms` on `r`; `at_ms >= last.at_ms + gap(..)` for both last movements on `r`; and for an ordinary arrival with a hold limit, `at_ms <= ready_ms + hold`. Nobody lands after the moment they would have diverted. |
| A31 | `perform` | POST | The flight's status went from `Queued` to exactly one of `Landed` or `Departed`. For a mayday arrival the new `inspection_until_ms` is greater than the old one and greater than `vacated_ms`. |
| A32 | `gap_behind` | POST | `result >= gap(r, flight, claim)`. |
| A33 | `hold_deadline` | POST | `Some` only for an ordinary arrival, and the addition does not overflow. |
| A34 | `divert_overdue` | LOOP | Every removed flight is an ordinary arrival with `ready_ms + hold <= now_ms`; `queue.remove` returning `None` is impossible (replace `else { continue }` with `expect`); the `Diverted.at_ms` is `<= now_ms`. |
| A35 | `end_name`, `busy_until`, `sealed` | PRE | `r < runways.len()`. Already enforced by indexing; no change needed. |
| A36 | `snapshot` | POST | `fingerprint.len() == 16`. |

### `scenario.rs`, `traffic.rs`, `policy.rs`

These are built by code, not by users, so assertions on their arguments are appropriate.

| ID | Function | Kind | Invariant |
|---|---|---|---|
| SC1 | `script` | POST | The `AdvanceTime` values are strictly increasing; the number of other commands equals `timed.len()`; and each of them comes after an `AdvanceTime` equal to its own `at_ms` (unless that is 0). |
| T1 | `traffic::generate` | PRE | If any flights are to be generated: `mix` is non-empty, `total_weight > 0` and `duration_ms > 0`. **Today** an empty mix silently generates A320s through the fallback at the end of `pick`. |
| T2 | `pick` | PRE | `roll < total weight`. The final line becomes `unreachable!`. |
| T3 | `traffic::generate` | POST | The result is sorted by `at_ms`, every `at_ms <= earliest_ms`, and flight identifiers are unique. |
| P1 | `policy::curfew` | PRE | `start_of_day_ms < MS_PER_DAY` and `end_of_day_ms <= MS_PER_DAY`. POST: `0 < length <= MS_PER_DAY`. |
| P2 | `policy::maintenance` | PRE | `duration_ms > 0`, and `every_ms == 0 \|\| every_ms >= duration_ms`. **Today** overlapping closures share one `Reason`, so the first `OpenRunway` reopens the runway in the middle of the second closure (probe: first 0, duration 100, every 60 leaves the runway open from 100 to 120). |
| P3 | `policy::wind_schedule` | PRE | Every direction `<= 360`. Otherwise the schedule is rejected at run time, one command at a time. |

## 8. `cli`, `wasm`, `web`

No assertions belong in `cli` or `wasm`: everything they handle is user input and already returns an error. Two notes:

- **`wasm::demo_scenario` takes the seed as `u32`** while the CLI takes `u64`. A seed above 4,294,967,295 printed by an overnight run cannot be typed into the browser, which is Phase 2's fifth finish-line item. Accept it as a string or a `BigInt`.
- In `web/src/timeline.ts`: `buildTimeline` should throw if a `diverted` output arrives while `airport.max_arrival_hold_ms` is `null` (today it quietly uses 0), and the running `count` of waiting flights should never be negative once all changes at one instant are applied.

## 9. What the assertions would catch on day one

Collected from above, most serious first. All confirmed on the scratch build.

1. **`EventQueue::push` panics on its first call** (Q1). The host cannot queue anything.
2. **A single well-formed command crashes the airport** in debug and corrupts it in release (X1, X2). In Phase 2 that would take down all three replicas at the same log position.
3. **A stale `VoteReply` is counted** by a candidate (N20), and so is one from a node outside the cluster (N5).
4. **A new node has no election deadline and no wake time** (N3, N4), so a cluster would sit silent for ever.
5. **`leader_hint` is set from any higher-term message**, including a candidate's `RequestVote` (N21).
6. **Overlapping maintenance closures reopen early** (P2), and **a curfew that starts after 24:00 is zero-length or panics** (P1, X5).
7. **`Input::Wake` is `todo!()`** (N35); `send_refusal`, `start_election`, `become_leader`, `append_entry` and `send_append_to_all` are stubs, and the last two take `&self` and return nothing, so they have no way to produce effects yet.

Three behaviours that are not assertion material but that you should decide on are in the test case document: a repeated `DeclareEmergency` silently reorders the queue, the result does depend on how often the clock ticks, and an inspection also blocks the crossing runway.
