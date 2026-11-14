# Airport Simulator: missing test cases

Companion to [invariants.md](invariants.md). Reviewed at commit `dead33d` plus the uncommitted host work.

## 1. Where coverage stands

I ran the suite on a scratch copy with Rust 1.97.0: all 78 Rust tests pass. I did not run the web tests.

| Area | Tests today | Gap |
|---|---|---|
| `airport` rules (`tests/rules.rs`, 41) | Strong, hand-worked | Edge cases and no-op commands; see section 6 |
| `airport` random checker (`tests/invariants.rs`, 2) | Five of the nine rules in design section 8 | "Emergencies first" is not checked; several cheap checks missing; see section 7 |
| `airport` unit tests (24) | Tables, wind, config, script | Table completeness, boundaries |
| `det` (6) | RNG pinned, FNV vectors | `below` not pinned, `Ms`, `objects` |
| `host` (2) | `run_scenario` only | `Clock`, `EventQueue`, `Seeds`, `mix`: **none** |
| `raft` (0) | — | Everything that exists is untested |
| `cli` (0) | Only through `web/src/sim.test.ts` | `parse`, `clock`, `report`, `describe`: **none** |
| `web` (3) | Native equals WebAssembly, one timeline run | `timeline.ts` logic only tested through a full demo run |

**Status column.** *Passes* means I ran the case as a probe on the scratch copy and the expected value below is what the code produced. *Red* means I ran it and the current code panics or gives a different answer; these are the tests that pay for themselves immediately. *Not run* means the expectation is worked from reading the code.

Test names follow the repo's style: a sentence that states the rule.

## 2. Three decisions to make before writing tests

Each of these is current behaviour that looks unintended. The test should assert whichever answer you pick.

**D-1. A repeated `DeclareEmergency` silently reorders the queue.** Declaring the same level again, or pan-pan after mayday, produces no output, but `declare_emergency` still removes and re-enqueues the flight with a new `seq`. Probe: X and Y both pan-pan with the same ready time, queue order X, Y. Send `DeclareEmergency(X, PanPan)` again: no output, the fingerprint changes, and the order is now Y, X. In Phase 2 this matters less than it would without duplicate filtering, but "a command that outputs nothing changes nothing" is true of every other no-op command (verified for six of them). Suggested: return before the remove when nothing changes. Cases AP-E1, AP-E2.

**D-2. The result does depend on how often the clock ticks.** The spec's review says it does not, and `the_rate_does_not_depend_on_how_often_the_clock_ticks` passes, but that test only floods one runway with identical flights. Counterexample, single runway: B77W lands at 0, a PC12 arrival ready at 0 (it must wait 363,269 ms behind the heavy), an A320 departure ready at 100,000.

| Clock | Order |
|---|---|
| 1 s ticks | L lands 0, **G departs 100,000**, F lands 363,269 |
| One step to 500,000 | L lands 0, F lands 363,269, **G departs 403,269** |

With fine ticks G slips into the gap ahead of F's claim. With one coarse step F's time has already passed when the queue is walked, so F is performed first and G queues behind it. On the random scenarios from `invariants.rs`, 228 of 300 seeds give different movements across tick sizes. Replication is unaffected, because every replica applies the same `AdvanceTime` commands. But Phase 3's controller chooses when to send them, so either the walk is made tick-proof (when a flight's time has passed, still let later flights slip in ahead of it if they fit), or the docs are corrected and the controller's tick becomes part of what defines a run. Case AP-Q1.

**D-3. A mayday inspection also blocks the crossing runway.** `runway_time` waits for `busy_until(c)` on every crossing runway, and `busy_until` includes the inspection. Probe on the demo airport: mayday lands on 27R at 0, inspection until 950,000; a gale at 1,000 makes 18 the only runway; an E190 ready at 1,000 lands on 18 at 950,000, not at 50,000 when 27R was physically clear. This may be what you want (the inspection vehicle is near the intersection), but no test or comment says so. Case AP-M3.

## 3. Phase 0: tooling

| ID | Test | How | Expect |
|---|---|---|---|
| P0-1 | The determinism ban really bans | A tiny fixture crate outside the workspace (`tests/ban-canary/`) that uses one of each banned item: `Instant::now`, `HashMap`, `thread::spawn`, `Mutex`, an `f64` addition. A `just check-ban` recipe runs clippy on it with the workspace `clippy.toml`. | Clippy fails, and the output contains one `disallowed` or `float_arithmetic` error per item. Guards against the ban list being silently disabled. |
| P0-2 | `wasm-bindgen` is pinned to one version everywhere | A shell or Bun test that extracts the version from `Cargo.toml`, `justfile` and `.github/workflows/ci.yml`. | All three equal. |
| P0-3 | The WebAssembly module exports exactly the documented functions | In `sim.test.ts`: list the function exports of the module. | `demoScenario`, `runScenario` (plus `default`/`initSync`). A new export must be a deliberate change. |
| P0-4 | Debug and release agree on overflow | After invariant G1: a `#[should_panic]` test for `Ms(u64::MAX) + Ms(1)`, run under both `cargo test` and `cargo test --release` in CI. | Panics in both. |

## 4. `det`

| ID | Test | Arrange and act | Expect | Status |
|---|---|---|---|---|
| DT-R1 | `below_sequence_is_pinned` | `Rng::new(42)`, eight calls to `below(10)` | `[0, 3, 6, 9, 9, 7, 7, 8]`. Traffic depends on this method, and only `next_u64` is pinned today. | Passes |
| DT-R2 | `below_one_is_always_zero` | 100 calls to `below(1)` | All 0 | Not run |
| DT-R3 | `below_is_roughly_uniform` | `Rng::new(7)`, 3,000 calls to `below(3)` | Counts 1021, 1034, 999 (assert each within 900..1100; the seed is fixed so it cannot flake) | Passes (Python port) |
| DT-R4 | `a_clone_continues_the_same_sequence` | Draw 5, clone, draw 5 from each | Equal | Not run |
| DT-R5 | `different_seeds_give_different_sequences` | Seeds 0..100, first value of each | All distinct | Not run |
| DT-F1 | `integers_hash_at_their_own_width` | `fingerprint(&1u32)` | `0xad2aca7747985764`, and differs from `fingerprint(&1u64)` | Passes |
| DT-F2 | `a_tuple_hashes_its_fields_in_order` | `fingerprint(&(1u8, 2u16))` | `0xd0aa6318672cf75e` (FNV-1a of bytes 01 02 00) | Passes |
| DT-F3 | `isize_hashes_like_i64` | `fingerprint(&-1isize)` | `0x8cf51a8bfca3883d`, equal to `fingerprint(&-1i64)` | Passes |
| DT-F4 | `a_vec_hashes_its_length_as_eight_bytes` | `fingerprint(&vec![1u8, 2])` | `0x9b19e1d327944d90`. This is the case that differs between a 32-bit and a 64-bit machine if `write_usize` is ever removed, so also add it to `sim.test.ts` if you export a helper. | Passes |
| DT-I1 | `ms_adds_and_displays` | `Ms(2) + Ms(3)`, `to_string`, `u64::from`, JSON | `Ms(5)`, `"5"`, `5`, `"5"` | Not run |
| DT-I2 | `ms_addition_panics_on_overflow` | `Ms(u64::MAX) + Ms(1)` | Panics (needs G1 or I1 for release) | Not run |
| DT-C1 | `default_config_leaves_room_for_heartbeats` | `Config::default()` | 50 and 150, and heartbeat < election base | Not run |
| DT-O1 | `requests_and_replies_are_told_apart` | `is_request` on all four `MessageCommand` variants | true, false, true, false | Not run |
| DT-O2 | `durable_and_message_keep_what_they_are_given` | `Durable::new`, `Message::new` | Fields equal the arguments | Not run |

## 5. `host`

`EventQueue`, `Clock`, `Seeds` and `mix` are complete enough to test now and have no tests.

### `clock.rs`

| ID | Test | Arrange and act | Expect | Status |
|---|---|---|---|---|
| H-C1 | `a_new_clock_reads_zero` | `Clock::default().now()` | `Ms(0)` | Not run |
| H-C2 | `the_clock_moves_forward_and_may_stand_still` | `advance_to(10)`, `advance_to(10)`, `advance_to(25)` | `now()` is 10, 10, 25 | Not run |
| H-C3 | `time_never_goes_backwards` | `advance_to(10)`, `advance_to(5)` | Panics (spec 11.10) | Not run |

### `queue.rs`

| ID | Test | Arrange and act | Expect | Status |
|---|---|---|---|---|
| H-Q1 | `events_come_out_in_time_order_with_ties_in_insertion_order` | Spec 11.10: push c@30, a1@10, b@20, a2@10, pop all | a1, a2, b, c | **Red**: the first `push` panics (invariant Q1) |
| H-Q2 | `push_returns_increasing_seqs` | Three pushes | Each `Seq` greater than the last | Red, same cause |
| H-Q3 | `peek_shows_the_next_pop_without_removing_it` | Push two, `peek`, `peek`, `pop` | Both peeks equal; the pop has the same time; one event remains | Red, same cause |
| H-Q4 | `an_empty_queue_peeks_and_pops_nothing` | New queue | `None`, `None`; `Default` behaves the same | Not run |
| H-Q5 | `an_event_pushed_at_the_time_just_popped_runs_after_its_peers` | Push x@10, y@10; pop (x); push z@10; pop, pop | y, then z | Red, same cause |
| H-Q6 | `nothing_can_be_scheduled_in_the_past` | Push @10, pop, push @5 | Panics (after invariant Q2) | Not run |
| H-Q7 | `the_queue_matches_a_sorted_list` | Property: seeded `Rng`, 1,000 random pushes and pops (push times at or after the last popped time). Reference model: a `Vec<(Ms, u64, E)>` stably sorted. | Every pop equals the model's | Not run |
| H-Q8 | `popped_events_leave_nothing_behind` | Push 100, pop 100 (only if `seqs` is kept) | Internal `seqs` is empty | Would be red: `seqs` is never pruned |

### `seeds.rs` and `hash.rs`

| ID | Test | Arrange and act | Expect | Status |
|---|---|---|---|---|
| H-S1 | `same_seed_same_numbers_and_streams_are_independent` | Spec 11.10 | As the spec | Not run |
| H-S2 | `streams_are_pinned` | `Seeds::new(42)`, first `next_u64` of `Network`, `Faults`, `Node { id: 1, life: 0 }` | `0x4c82d77d9955b6db`, `0xfbec13dbcd08a352`, `0xd032dfb4c046ce07`. A changed value means every recorded seed replays differently. | Passes |
| H-S3 | `asking_for_one_stream_does_not_disturb_another` | `rng(Network)` before and after `rng(Faults)` and `rng(Node..)` | Equal (the method takes `&self`, so this documents intent) | Not run |
| H-S4 | `no_two_streams_share_a_seed` | Roots 0..8 × {Network, Faults, Node id 0..5 × life 0..4} | All 176 derived seeds distinct. In particular `Node {id: 1, life: 0}` differs from `Node {id: 0, life: 1}`. | Passes (Python port of `mix`) |
| H-S5 | `each_life_of_a_node_gets_fresh_numbers` | `Node {id: 2, life: 0}` against `life: 1` | Different | Not run |
| H-S6 | ~~`children_get_their_own_seeds`~~ | Removed: `Seeds::child` went with the nested hosts in the second revision of the Phase 2 spec. | — | Retired |
| H-M1 | `mix_is_pinned` | `mix(42, 7)`, `mix(7, 42)`, `mix(42, 8)`, `mix(0, 1)`, `mix(u64::MAX, u64::MAX)` | `0x2a08f0a6af27f33b`, `0xd4c07636ab140bf0`, `0xd1116591326a042b`, `0xe2f3b303c39f5e98`, `0x949cf74af653602a` | First value passes in Rust; the rest from the Python port that matched it |
| H-M2 | `mix_is_not_symmetric` | The first two of H-M1 | Different | Passes |
| H-M3 | `mix_of_zero_and_zero_is_zero` | `mix(0, 0)` | 0. Harmless (`Rng::new(0)` is valid), but worth knowing. | Passes |
| H-M4 | `one_flipped_bit_changes_about_half_the_output` | For 64 single-bit flips of `b` in `mix(1, b)` | Each flips between 16 and 48 output bits | Not run |

The doctest on `mix` redefines `mix` as `a ^ b` inside the example, because the function is private, so it tests nothing. Replace it with H-M1 and H-M2.

### `lib.rs`

| ID | Test | Arrange and act | Expect | Status |
|---|---|---|---|---|
| H-L1 | `the_demo_json_is_the_demo_scenario` | Parse `demo_scenario_json(DemoOptions::default())` | Equals `scenario::demo(1, 6, 40, 40)`: 3 `set_wind`, 1 close, 1 open, 1 emergency, 480 requests, last `advance_time` at 28,800,000 | Passes |
| H-L2 | `zero_hours_means_one_hour` | `hours: 0` against `hours: 1` | Equal JSON | Passes |
| H-L3 | `a_demo_with_no_traffic_still_runs` | `arrivals_per_hour: 0`, `departures_per_hour: 0` | No emergency command; all stats 0 | Passes |
| H-L4 | `json_of_the_wrong_shape_is_an_invalid_scenario` | `run_scenario_json("{}")`, `"[]"`, `""` | Each starts with `invalid scenario` | Not run |
| H-L5 | `each_bad_airport_names_its_problem` | Scenarios with a duplicate end name, a heading of 360, an unknown crossing, 13 runways | Each starts with `invalid airport:` and contains the matching `ConfigError` text | Not run |
| H-L6 | `the_two_entry_points_agree` | `run_scenario` then serialise, against `run_scenario_json` | Equal strings | Not run |

## 6. `raft`

`Member` (formerly `BrowserNode`) keeps its fields private, so these go in a `#[cfg(test)] mod tests` inside `member.rs` (or make it `pub`, which spec 11.11 needs anyway). The tests for the unwritten parts are already listed in the spec, tickets B2 to B6; the cases here are for what exists, plus additions the spec does not list.

A helper worth writing first: `fn member(term: u64) -> Member<u8>` with id 1 and peers 2 and 3, and `fn msg(from, term, command)`.

| ID | Test | Arrange and act | Expect | Status |
|---|---|---|---|---|
| RF-1 | `a_majority_is_more_than_half` | Clusters of 1 to 6 nodes | `majority()` is 1, 2, 2, 3, 3, 4 | Passes |
| RF-2 | `a_node_starts_as_a_follower_whatever_it_was_before` | `new` with `Durable { term: 7, voted_for: Some(2), log: [..] }` | Follower; `log()` returns the log; `into_durable()` returns the same three values (B1 "done when") | Not run |
| RF-3 | `a_new_node_asks_to_be_woken` | `new(.., now = Ms(100))` | `next_wake()` is `Some(t)` with 250 <= t <= 400 for the default config | **Red**: returns `None` |
| RF-4 | `a_wake_at_any_time_is_harmless` | `step(now, Wake)` before the deadline | No effects, same durable state | **Red**: `todo!()` |
| RF-5 | `a_follower_refuses_a_proposal` | `step(Propose(7))` on a new node | `[NotLeader { hint: None }]`, log unchanged | Passes |
| RF-6 | `a_higher_term_is_adopted_before_the_message_is_handled` | Node in term 0 receives `RequestVote` from 2 in term 1 | Term 1, `voted_for == Some(2)`, one `VoteReply` to 2 carrying term 1 | Passes |
| RF-7 | `a_vote_is_granted_again_to_the_same_candidate_but_not_to_another` | After RF-6: `RequestVote` term 1 from 3, then again from 2 | No effects for 3; one `VoteReply` for 2; `voted_for` still 2 | Passes |
| RF-8 | `a_candidates_request_does_not_make_it_the_leader_hint` | After RF-6: `Propose` | `NotLeader { hint: None }` | **Red**: hint is `Some(2)` |
| RF-9 | `a_stale_request_is_refused_with_our_term` | Node in term 5 receives `RequestVote` in term 3 | One refusal to the sender carrying term 5; term and vote unchanged | **Red**: `send_refusal` unimplemented |
| RF-10 | `a_stale_reply_changes_nothing` | Candidate in term 5 with its own vote receives `VoteReply` in term 3 | No effects, `votes.len() == 1`, still candidate | **Red**: the vote is counted and the node tries to become leader |
| RF-11 | `the_same_vote_twice_counts_once` | Candidate in a five-node cluster receives `VoteReply` from 2 twice | `votes.len() == 2`, still candidate | Not run (set semantics; should pass) |
| RF-12 | `a_follower_ignores_vote_replies` | Follower receives a same-term `VoteReply` | No effects, no change | Not run |
| RF-13 | `a_message_from_outside_the_cluster_is_a_bug` | Any message with `from: NodeId(99)`, or `from` equal to the node's own id | Panics (after invariant N19) | **Red**: a vote from 99 is counted |
| RF-14 | `a_vote_reply_must_say_whether_it_was_granted` | Design note, not a test yet: `VoteReply` has no `granted` flag and `RequestVote` carries no last index or term, so a refusal cannot be told from a grant and logs cannot be compared (B2). Add the fields, then test both branches. | | |

Additions to the spec's B-ticket tests, for when the code exists:

| ID | Ticket | Test |
|---|---|---|
| RF-15 | B2 | A cluster of four needs three votes: two votes do not make a leader. |
| RF-16 | B2 | A refused vote does not reset the election deadline; a granted one does (`next_wake` before and after). |
| RF-17 | B3 | The same `AppendEntries` delivered twice leaves the log unchanged the second time and gets the same reply. |
| RF-18 | B4 | An older, shorter `AppendEntries` that arrives after a newer, longer one does not shorten the log (E4 switch 5, as a unit test). |
| RF-19 | B5 | A late `AppendReply` reporting a smaller match than already recorded does not move `matched` back. |
| RF-20 | B5 | `Committed` effects across three consecutive steps have consecutive indices with no gap and no repeat. |
| RF-21 | B1 | `next_wake` called twice returns the same value, and stepping with `Wake` at exactly that time does something (the deadline is inclusive: `now >= d`). |

## 7. `airport`: unit tests

### `data.rs`

| ID | Test | Expect | Status |
|---|---|---|---|
| AP-D1 | `every_pair_in_the_approach_table` | All 16 `(leader, follower)` pairs against a literal 4×4 array with the radar minimum applied. Today 13 are tested; `(Light, Medium)`, `(Light, Super)` and `(Medium, Super)` are not. | Not run |
| AP-D2 | `every_pair_in_the_departure_table` | All 16 pairs. Today 7 are tested; `(Super, Light)` = 180,000 and the whole Light row = 60,000 are not. | Not run |
| AP-D3 | `occupancy_by_class` | Arrival 40, 50, 55, 60 s and departure 30, 40, 50, 55 s for Light to Super | Not run |
| AP-D4 | `the_sine_table_is_right` | Spot values 30° = 5000, 45° = 7071, 60° = 8660; never decreasing; and for every d, `sin²(d) + sin²(90 − d)` is within 15,000 of 10⁸ (the largest actual error is 11,286). Catches a mistyped entry. | Passes (checked all 91 entries against `math.sin`) |
| AP-D5 | `an_unknown_aircraft_index_panics` | `aircraft(14)` panics | Not run |
| AP-D6 | `aircraft_codes_are_unique_and_the_default_mix_knows_them_all` | No repeated `icao`; every type in `default_mix()` is in the table; the weights sum to 100 | Not run |

### `separation.rs` and `wind.rs`

| ID | Test | Arrange | Expect | Status |
|---|---|---|---|---|
| AP-S1 | `equal_speeds_use_the_simple_gap` | B788 behind B763, both 140 kt, heavy behind heavy: 4 nm | 4 / 140 h = 102,857.1, so 102,858 | Passes |
| AP-S2 | `with_no_final_approach_a_slow_follower_needs_only_the_plain_gap` | AT76 behind B738, final of 0 nm | 3 / 113 h = 95,575.2, so 95,576, equal to `gap_ms(3, 113)` | Passes |
| AP-S3 | `the_slowest_behind_the_biggest` | C172 (65 kt) behind A388 (138 kt), 8 nm, 6 nm final: 14 / 65 h − 6 / 138 h | 618,863 | Passes |
| AP-S4 | `a_longer_final_widens_the_gap_only_for_a_slower_follower` | AT76 behind B738 at finals 3, 6, 12; B738 behind AT76 at the same | Strictly increasing; constant at 75,000 | Not run |
| AP-S5 | `no_gap_is_shorter_than_the_separation_at_the_followers_speed` | Property: all 14 × 14 pairs, finals 0..=15 | `approach_gap_ms >= gap_ms(sep, vf)` and `> 0` | Not run |
| AP-S6 | `gap_ms_rounds_up_and_does_not_overflow` | `gap_ms(1, 7)`, `gap_ms(3, 144)`, `gap_ms(u32::MAX, 1)` | 514,286; 75,000 exactly; `Some(u32::MAX * 3_600_000)` | First two pass (Python) |
| AP-W1 | `forty_five_degrees_splits_the_wind_evenly` | Wind 045 at 20 kt, runway heading 090 | Headwind and crosswind both 141,420 | Passes |
| AP-W2 | `a_wind_from_behind_is_a_negative_headwind` | Wind 240 at 10, heading 090 (150° off) | Headwind −86,600; crosswind 50,000 | Not run |
| AP-W3 | `the_opposite_end_sees_the_same_crosswind_and_the_opposite_headwind` | Property: every direction 0..360, headings 0..360 in steps of 5, speed 17 | `components(w, h)` and `components(w, (h + 180) % 360)`: equal crosswind, negated headwind; both within `speed * 10_000` | Not run |
| AP-W4 | `the_strongest_wind_does_not_overflow` | Speed `u16::MAX`, 45° off | 463,397,985 for both | Passes |
| AP-W5 | `calm_has_no_components` | Speed 0, any direction | 0 and 0 | Not run |

### `config.rs`

| ID | Test | Expect | Status |
|---|---|---|---|
| AP-C1 | `twelve_runways_are_allowed_and_thirteen_are_not` | 12 parallel runways: valid and all 12 active; 13: `TooManyRunways` | Passes |
| AP-C2 | `a_heading_must_be_below_360` | 359 valid; 360 gives `BadHeading("09L/27R")` | Passes for 360 |
| AP-C3 | `a_runway_cannot_cross_itself_under_another_name` | Crossing `("09L", "27R")` gives `SelfCrossing("09L")` | Passes |
| AP-C4 | `both_ends_of_one_runway_need_different_names` | Runway `09`/`09` gives `DuplicateRunwayName("09")` | Passes |
| AP-C5 | `a_crossing_listed_twice_is_the_same_as_once` | Masks equal to the single listing; also when named by full name or by the other end | Not run |
| AP-C6 | `every_config_error_reads_as_a_sentence` | The six `Display` strings, literally | Not run |
| AP-C7 | `a_full_name_must_be_low_then_high` | `is_named("27R/09L")` is false; `is_named("")` is false | Not run |
| AP-C8 | `a_null_hold_limit_means_wait_for_ever` | JSON with `"max_arrival_hold_ms": null` gives `None`; absent gives 45 minutes; `exit_extra_ms` absent gives 0 | Not run |
| AP-C9 | `no_runways_is_reported_before_anything_else` | Empty runway list plus a crossing naming nothing: `NoRunways` | Not run |

### `policy.rs`, `scenario.rs`, `traffic.rs`

| ID | Test | Arrange | Expect | Status |
|---|---|---|---|---|
| AP-P1 | `a_daytime_curfew_stays_inside_the_day` | `curfew(1 h, 5 h, 1 day)` | Stop at 1 h, lift at 5 h | Passes |
| AP-P2 | `equal_start_and_end_is_a_full_day` | `curfew(0, 0, 1 day)` | Stop at 0, lift at 24 h | Passes |
| AP-P3 | `no_curfew_starts_at_or_after_the_limit` | `curfew(1 day, 1 day, 1 day)`; `until_ms = 0` | Empty | Passes |
| AP-P4 | `a_curfew_start_must_be_a_time_of_day` | `curfew(31 h, 6 h, ..)` | Panics with a message (invariant P1) | Red today in the sense that it panics with a bare overflow; 30 h gives a zero-length curfew |
| AP-P5 | `maintenance_closures_must_not_overlap` | `maintenance("09", 0, 100, 60, 150)` | Panics (invariant P2). Today, run through an airport, it gives closed 0–100, open 100–120, closed 120–160. | Red |
| AP-P6 | `maintenance_that_would_start_at_the_limit_is_left_out` | `first_ms == until_ms` | Empty | Not run |
| AP-P7 | `a_wind_schedule_keeps_its_order` | Three changes; empty slice | Three `SetWind` at the given times, in order; empty | Not run |
| AP-SC1 | `the_last_tick_stops_at_the_end` | `script([], 10, 25)` | t10, t20, t25 | Passes |
| AP-SC2 | `a_zero_tick_is_treated_as_one` | `script([], 0, 3)` | t1, t2, t3 | Passes |
| AP-SC3 | `nothing_in_nothing_out` | `script([], 10, 0)` | Empty | Passes |
| AP-SC4 | `a_command_after_the_end_is_still_sent` | One command at 50, end 25, tick 10 | t10 … t50, then the command | Passes |
| AP-SC5 | `commands_at_the_same_moment_keep_their_given_order` | Two *different* commands at t = 25. The existing test uses the same command twice, so it cannot tell a stable sort from an unstable one. | Given order kept | Not run |
| AP-SC6 | `time_in_a_script_only_moves_forward` | Property over the random scenarios | `AdvanceTime` values strictly increase; every other command follows an `AdvanceTime` equal to its time | Not run |
| AP-SC7 | `an_empty_scenario_ends_where_it_starts` | `run` with no commands | `start == end`, no outputs | Not run |
| AP-SC8 | `the_demo_never_sends_a_command_that_is_refused` | `demo(seed, 6, 45, 45)` for seeds 0..40 | `stats.rejected == 0` | Passes |
| AP-SC9 | `stats_add_up_to_the_outputs` | Demo seed 1 | Sum of `Landed.delay_ms` equals `arrival_delay_ms`; same for departures | Passes |
| AP-T1 | `the_flight_count_rounds_to_nearest` | 1 an hour for 30 minutes; for 29 minutes; any rate for zero duration | 1; 0; 0 | Passes |
| AP-T2 | `an_empty_mix_is_a_bug` | `mix = vec![]` with 5 flights | Panics (invariant T1). Today: five A320s. | Red |
| AP-T3 | `flights_are_numbered_from_100_arrivals_first` | 3 arrivals, 2 departures | Identifiers end 100..104, unique; the three landings carry 100 to 102 | Passes |
| AP-T4 | `notice_longer_than_the_wait_files_at_time_zero` | `notice_ms = u64::MAX` | Every `at_ms == 0` | Passes |
| AP-T5 | `requested_times_fall_in_the_window` | `start_ms = 1_000_000`, one hour | Every `earliest_ms` in `start..start + duration` | Not run |
| AP-T6 | `a_single_type_mix_and_a_zero_weight` | Mix `[("A320", 0), ("B738", 5)]` | Every flight a B738 | Not run |
| AP-T7 | `pick_lands_on_the_right_side_of_each_boundary` | Mix weights 3 and 2; rolls 0, 2, 3, 4 | First, first, second, second | Not run |

## 8. `airport`: state machine (component tests for `tests/rules.rs`)

| ID | Test | Arrange and act | Expect | Status |
|---|---|---|---|---|
| AP-N1 | `a_command_that_changes_nothing_says_nothing_and_leaves_no_trace` | On the demo airport with runway 18 closed for storm, a storm ground stop and wind 000/5: send close 18 for storm again, open 18 for *other*, ground stop storm again, lift ground stop *other*, wind 360/5, `AdvanceTime` to the current time | Each returns no outputs and leaves the fingerprint unchanged | Passes (all six) |
| AP-N2 | `undoing_a_change_restores_the_fingerprint` | Close then open 18 (by the other end's name); ground stop then lift; close then open an active runway | Fingerprint equals the starting one after each pair | Passes |
| AP-N3 | `advancing_to_the_same_time_twice_does_nothing_more` | Five A320s queued, `AdvanceTime` 100,000 twice | Second call: no outputs, same fingerprint | Passes |
| AP-E1 | `declaring_the_same_emergency_again_changes_nothing` | Decision D-1. X and Y pan-pan, same ready time; declare X pan-pan again | No output, same fingerprint, order still X, Y | **Red** |
| AP-E2 | `a_downgrade_changes_nothing` | X mayday, then declare X pan-pan | No output, same fingerprint | **Red** |
| AP-E3 | `a_pan_pan_goes_first_but_closes_nothing` | Single runway: A then P requested for time 0, P declares pan-pan, advance | P lands at 0, A at 78,833; no `RunwayInspection` | Passes |
| AP-M1 | `no_inspection_when_the_airport_sets_none` | `mayday_inspection_ms = 0`, mayday lands | One `Landed` with `emergency: Some(Mayday)`, no `RunwayInspection`, next arrival at the normal gap | Passes (first part) |
| AP-M2 | `a_mayday_departure_closes_nothing` | Departure declares mayday | `Departed` at 0 with the emergency; no inspection | Passes |
| AP-M3 | `an_inspection_holds_the_crossing_runway_too` (or the opposite) | Decision D-3, as described in section 2 | E190 lands on 18 at 950,000 today | Passes as written; flip if you decide otherwise |
| AP-H1 | `a_flight_may_land_exactly_at_its_limit_but_not_after` | Single runway, hold limit 157,666 ms (two A320 gaps), five A320s ready at 0, 1 s ticks | F0, F1, F2 land at 0, 78,833 and 157,666; F3 and F4 divert at 157,666 | Passes |
| AP-H2 | `diversion_times_do_not_depend_on_the_tick` | The same in one step to 400,000 | Identical outputs | Passes |
| AP-H3 | `arrivals_divert_during_a_ground_stop` | Demo airport, ground stop, A320 ready at 0, advance to 46 minutes | One `Diverted` at 2,700,000 | Passes |
| AP-H4 | `a_flight_about_to_divert_holds_no_one_else_up` | Demo airport, both long runways closed; an A388 and an E190 ready at 0 | The E190 lands on 18 at 0; the A388 is still queued | Passes |
| AP-R1 | `a_rejection_changes_only_the_rejection_count` | Property over the random scenarios: clone the airport before each command; if the output is a single `Rejected`, bump `rejected` on the clone | Clone equals the airport (`==`, not just fingerprint) | Not run |
| AP-R2 | `a_request_for_the_past_is_ready_now` | Clock at 100,000; request landing with `earliest_ms: 5` | Lands at 100,000 with `delay_ms: 0` | Passes |
| AP-R3 | `no_command_can_crash_the_airport` | Property: random commands with extreme values (`earliest_ms` and `now_ms` up to `u64::MAX`, speed `u16::MAX`, empty and very long strings, unknown names) on random airports | Never panics; every refusal is a `Rejected` | **Red**: `earliest_ms = u64::MAX` then any `AdvanceTime` overflows (invariants X1, X2) |
| AP-WD1 | `every_runway_out_of_limits_stops_the_airport_until_the_wind_drops` | Single runway 09/27, wind 000 at 26 kt; an A320 ready at 0; advance to 600,000; wind 000 at 25; advance | `ActiveRunways []` at 0; nothing moves; then `ActiveRunways ["09"]` and a landing at 600,000 | Passes |
| AP-WD2 | `a_crosswind_exactly_on_the_limit_is_allowed` | Wind 000 at 25 kt on 09/27 | Only `WindChanged`; the runway stays active | Passes |
| AP-WD3 | `turning_the_runway_round_does_not_reset_separation` | Single runway, wind 270/10, A and B (A320) ready at 0; A lands on 27 at 0; at 1,000 wind 090/10 | `ActiveRunways ["09"]`; B lands on **09** at 78,833 | Passes |
| AP-WD4 | `equal_crosswinds_keep_the_first_listed_runway` | Two crossing runways, headings 045 and 135, same length. Wind 090/10 (45° off both), then 100/10 | Stays on A1; then switches to B1 (35° off beats 55°) | Passes |
| AP-WD5 | `in_calm_air_the_demo_uses_the_low_ends_of_both_parallels` | `Airport::new(demo())` | `["09L", "09R"]` | Passes |
| AP-G1 | `movements_resume_only_when_every_ground_stop_is_lifted` | Storm and curfew stops; A320 ready at 0; lift storm at 10,000; lift curfew at 20,000 | `clear: false`, then `clear: true`, then a landing at 20,000 with `delay_ms` 20,000 | Passes |
| AP-G2 | `nothing_moves_during_a_curfew` | Component test through `policy::curfew(23 h, 6 h, 2 days)` and `script`, single runway, one A320 departure wanting every hour at half past for 48 hours | All 48 depart; none between 23:00 and 06:00 on either night (the first six, before the first curfew, are unaffected); the queue is empty at the end. Design section 8 names curfews explicitly and only raw ground stops are tested. | Passes |
| AP-Q1 | `the_outcome_does_not_depend_on_the_tick` (or a test that pins the difference) | Decision D-2. The three-flight counterexample from section 2, and the property over random scenarios at ticks of 1 s, 5 s, 30 s, 4 min, 1 h | Same movements at every tick | **Red**: 228 of 300 seeds differ |
| AP-SN1 | `a_snapshot_describes_the_state` | Demo airport; B then A request landing; B declares pan-pan | Queue lists B (pan-pan) before A; runway 18 has `active_end: null`; `inspection_until_ms` is null unless it is in the future; `fingerprint` is 16 hex digits equal to `format!("{:016x}", fingerprint())` | Passes (queue order and shape) |
| AP-SN2 | `a_run_result_survives_json` | Serialise and parse a `RunResult` | Equal | Not run |
| AP-CL1 | `a_clone_is_independent` | Clone, apply a command to the clone | Original fingerprint unchanged; `clone == original` before, not after | Not run |

## 9. `airport`: the random checker (`tests/invariants.rs`)

**Rules to add to `check`.** Each uses only the outputs and the scenario, none of the airport's logic.

| ID | Rule | How to check it |
|---|---|---|
| CK-1 | **Emergencies first** (design section 8; not checked today) | Record when each emergency was declared. When an emergency flight E moves on runway `r` at `t_E`, look at the previous movement P on `r`. If P was an ordinary flight and `P.at_ms` is after E's declaration, then P must not have delayed E: `t_E` must be no later than the earliest E could have gone had P not moved, which the checker can work out from what it already tracks (E's ready time, the declaration time, the movement before P plus its gap, `inspection_until`, `open_since`, `ground_clear_since`, crossing runways' `vacated_ms`). A one-sided bound, but it catches any ordinary flight slotted in at an emergency's expense. |
| CK-2 | Nobody moves before they asked | `at_ms >= earliest_ms` from the request, and `at_ms >=` the `Accepted.at_ms`. `delay_ms == at_ms − max(earliest_ms, accepted_at)`. |
| CK-3 | Hold limit | An ordinary arrival's `delay_ms <= max_arrival_hold_ms`. A `Diverted.at_ms` is exactly ready time plus the limit. No flight that declared an emergency before that moment diverts. No departure diverts. |
| CK-4 | Outputs are not from the future | Check per command: every `at_ms <= ` the latest `AdvanceTime` so far, and every movement's `at_ms >=` the `AdvanceTime` before that. |
| CK-5 | Inspections | A `RunwayInspection` appears exactly when a mayday arrival lands and the configured inspection is above zero, with `from_ms == vacated_ms` and `until_ms == from_ms + mayday_inspection_ms`. |
| CK-6 | Totals | `stats.arrival_delay_ms` and `stats.departure_delay_ms` equal the sums over the outputs; `stats.rejected` equals the number of `Rejected` outputs. |
| CK-7 | Whole-state invariants | Call `airport.check_invariants()` (see the invariants document, A1 to A12) after every command. |

**Things the generator never produces.** Widening it costs a few lines each.

| ID | Missing input | Why it matters |
|---|---|---|
| GN-1 | More than 4 runways (up to 12) | `reconfigure` searches 2ⁿ sets; only 16 are ever exercised. |
| GN-2 | Unknown flight, runway and aircraft names; wind above 360 | Rejection paths are only unit tested. |
| GN-3 | `DeclareEmergency` repeated or downgraded | Decision D-1. |
| GN-4 | An `AdvanceTime` that goes backwards, dropped in at random | "Time never goes backwards" is only unit tested. |
| GN-5 | Curfews and maintenance from `policy`, mixed into the random commands | They are the proposal's "small generators of commands" and are otherwise only unit tested. |
| GN-6 | A second checker test that needs no faults to clear: stop the run at a random point and check every rule except "no lost flights" | The drain at the end can hide a rule broken only transiently. The rules are all per-movement, so they hold at any cut. |

## 10. `cli`

`parse`, `run`, `clock`, `report` and `describe` are private functions in `main.rs` with no tests. Add a `#[cfg(test)] mod tests` there, plus one integration test in `crates/cli/tests/` that runs the binary through `env!("CARGO_BIN_EXE_airport-sim")`. None of these were run; expectations are from reading the code.

| ID | Test | Arrange | Expect |
|---|---|---|---|
| CL-1 | `clock_formats_simulated_time` | 0; 999; 3,661,000; 86,399,999; 360,000,000 | `00:00:00`; `00:00:00`; `01:01:01`; `23:59:59`; `100:00:00` (hours are not wrapped at a day) |
| CL-2 | `options_have_defaults` | `parse(&[])` | Seed 1, 6 hours, 40 and 40, no flags, no file |
| CL-3 | `each_option_is_read` | `--seed 7 --hours 2 --arrivals 10 --departures 5 --json --quiet file.json` | All set |
| CL-4 | `a_number_option_needs_a_number` | `--seed`; `--seed x`; `--seed -5`; `--seed --json` | Each is an error starting `--seed needs a whole number` |
| CL-5 | `an_hour_count_must_fit` | `--hours 4294967296` | Error `4294967296 is too large` |
| CL-6 | `unknown_and_surplus_arguments_are_refused` | `--bogus`; `a.json b.json` | `unexpected argument --bogus`; `unexpected argument b.json` |
| CL-7 | `a_dash_means_standard_input` | `-` | `file == Some("-")` |
| CL-8 | `no_command_or_an_unknown_one_prints_usage` | `run(&[])`; `run(&["fly"])` | `Err(USAGE)` |
| CL-9 | `gen_prints_the_scenario_the_host_builds` | `run(&["gen", "--seed", "7"])` | `host::demo_scenario_json(seed 7)` plus a newline |
| CL-10 | `run_needs_a_file_that_exists` | `run(&["run"])`; `run(&["run", "/no/such/file"])` | `run needs a scenario file…`; `cannot read /no/such/file: …` |
| CL-11 | `demo_and_gen_then_run_agree` | Write `gen --seed 7` to a temporary file, `run <file> --json`; compare with `demo --seed 7 --json` | Equal |
| CL-12 | `quiet_prints_only_the_summary` | `demo --quiet` | Exactly eight lines, the first starting `ended at`, the last `fingerprint  ` followed by 16 hex digits |
| CL-13 | `a_mean_delay_with_no_flights_is_zero` | `report` on a `RunResult` with zero stats | Contains `landed       0 (mean delay 0 s)` and does not panic |
| CL-14 | `every_output_has_its_line` | `describe` once per `Output` variant | `Accepted` gives `None`. Landed with mayday: `00:01:00 BAW1 (A320) landed on 27L, delayed 12 s [mayday]`. Pan-pan tag is ` [panpan]`. Wind 5° at 10 kt: `00:00:00 wind 005 at 10 kt`. `RunwayOpened { open: false }` contains `still closed`. `GroundStopLifted { clear: false }` ends `, another remains`. Rejected shows the reason in debug form, e.g. `rejected: DuplicateFlight`. |
| CL-15 | `the_program_exits_with_the_right_code` | Binary with no arguments; with `demo --quiet` | Exit 1 with usage on standard error and nothing on standard output; exit 0 |

## 11. `web`

`buildTimeline` is pure, so it can be tested with small hand-made inputs and no WebAssembly. That is faster and far more specific than the one full-demo test. Export `subtract` (or test it through `buildTimeline`). Not run.

| ID | Test | Input | Expect |
|---|---|---|---|
| WB-1 | `clock agrees with the command line` | The values in CL-1 | The same strings |
| WB-2 | `subtract cuts holes out of a band` | Band 0–100 with holes: 40–60; 0–100; −10–20; 90–200; 100–150; 0–0; two holes 10–20 and 30–40 | Two parts; none; 20–100; 0–90; unchanged; unchanged; three parts |
| WB-3 | `a landing becomes a mark on the right lane` | One `landed` on `27L` | A mark in lane `09R/27L` with `end: "27L"` |
| WB-4 | `a flight that never waited is never counted as waiting` | One movement with `delay_ms: 0` | `maxWaiting` is 0 and no entry in `waiting` is negative |
| WB-5 | `a closure lasts until the last reason is lifted` | closed (storm) at 10, closed (maintenance) at 20, opened `open: false` at 30, opened `open: true` at 40 | One band 10–40 labelled `closed: storm` |
| WB-6 | `a closure still in force runs to the end` | closed at 10, run ends at 100 | Band 10–100 |
| WB-7 | `a runway unused at the start is shaded from zero` | Start snapshot with `active_end: null`, `active_runways` including it at 50 | `unused` band 0–50 |
| WB-8 | `a closed runway shows the closure, not "not in use"` | Unused 0–100 and closed 20–40 on one lane | `unused` 0–20, `closed` 20–40, `unused` 40–100 |
| WB-9 | `a zero-length band is dropped` | closed and opened at the same instant | No band |
| WB-10 | `ground stops become stop bands` | started 10, lifted `clear: false` 20, lifted `clear: true` 30; and one never lifted | 10–30; start to `endMs` |
| WB-11 | `an output for an unknown runway is an error` | `landed` on `99` | Throws `unknown runway 99` |
| WB-12 | `wind notes are padded to three digits` | `wind_changed` 5° at 10 kt | `wind 005°/10 kt` |
| WB-13 | `runScenario reports an invalid airport` | A scenario with no runways, through the WebAssembly build | Throws `invalid airport` (only `invalid scenario` is tested today) |
| WB-14 | `a seed too large for the browser is refused, not truncated` | `demoScenario(2 ** 32 + 1, …)` | From how `wasm-bindgen` passes a `u32`, I expect it to be cut to 32 bits and run seed 1; I did not run this. Decide, then test. See the note on `u32` seeds in the invariants document. |

## 12. Suggested order

1. **The red tests** (H-Q1, AP-R3, RF-8 to RF-10, RF-13, AP-P5, AP-T2): each one is a real fault, and each has a one-line assertion in the invariants document that would have caught it.
2. **Decisions D-1 to D-3**, then AP-E1, AP-E2, AP-Q1, AP-M3.
3. **`host` C1 pieces** (section 5): they are what you are building right now.
4. **Checker additions** CK-1 to CK-7, since Phase 2's "same answer as one airport" rule leans entirely on Phase 1's checker being complete.
5. Everything else, cheapest first: `det`, `cli`, table completeness, `timeline.ts`.
