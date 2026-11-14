# Phase 2 spec: nodes, the host that simulates them, and three replicas that agree

This is the working document for Phase 2 of Airport Simulator. It has two parts: a short review of the repository, and a list of tickets for building Phase 2.

It was revised once after work began, to change what a "node" is and to make every participant something that could run on its own machine. Section 2 says what changed and where the code stands. The tickets on GitHub (milestone "Phase 2") carry the same text as section 7.

## 1. How to use this document

- **Each ticket stands on its own.** It says what to build, the rules it must obey, and how you know it is finished. You should not need to scroll back to understand one.
- **Each ticket is marked "by hand" or "fine to delegate".** By hand means it is distributed-systems logic and the point of the project. Fine to delegate means plumbing that Claude Code can do without taking any of the learning away.
- **The tickets give rules and tests, not solutions.** Where a ticket lists rules, they are the published rules of Raft, restated in this project's words. How to structure the code that obeys them is left to you.
- **One section is marked as a spoiler.** Ticket E4 names the classic mistakes people make when writing Raft. Skip that list until your implementation passes everything else, if you would rather find them yourself.

## 2. Review of the repository so far

I read every file in the repository at commit `0190ecf`. I could not run the tests, because the pinned Rust 1.97 toolchain could not be downloaded where I was working, so this review is from reading the code, not from running it.

### What is in good shape

- **The airport is already the right shape for replication.** `Airport::apply(&Command) -> Vec<Output>` is the whole interface, and time only moves through `Command::AdvanceTime`. That is exactly what a replicated log needs to feed: one command in, a new state and some outputs out.
- **Determinism is enforced, not just intended.** The Clippy ban list, the hand-written random number generator in `rng.rs`, the fixed-width fingerprint in `fingerprint.rs`, and the web test that runs the same scenario natively and as WebAssembly and demands identical output are all in place.
- **The tests are the right kind.** `tests/rules.rs` shows its arithmetic, and `tests/invariants.rs` uses a checker that shares the data tables with the airport but none of its logic. Phase 2 uses that same idea for Raft.
- **Time is just another command.** `AdvanceTime` goes through the log like everything else, so a delay in replicating a command cannot change what the airport decides, only when the replicas hear about it. Note that the result *does* depend on how often the clock ticks (see `the_tick_is_part_of_the_run` in `tests/rules.rs`): a coarse step can let a flight whose slot has already passed go before a later flight that a finer tick would have slipped in ahead of it. So the controller's tick is part of what defines a run, and Phase 3's controller must choose it deliberately.
- **Phase 1's finish line in the design proposal is met**, as far as reading can tell: same fingerprint in both builds, hand-worked rates for a flooded runway, and tests for all four modelled aspects of aviation.

### Six things Phase 2 has to deal with

These are not faults in Phase 1. They are the places where the existing code meets the new work.

1. **The random number generator and the fingerprint live in the `airport` crate.** The Raft crate needs random numbers (for election timing) and the host needs both, but Raft should not have to depend on airports. Ticket A1 moves them to a small shared crate.
2. **Commands carry no sender and no sequence number.** A resent command would be applied twice. The airport half-defends itself today: a second `RequestLanding` for the same flight is answered with `Rejected: DuplicateFlight`. But that still changes the state (the rejected count goes up, so the fingerprint changes) and gives the sender the wrong answer. Ticket D2 fixes this outside the airport, as the design proposal says.
3. **`Airport` can be cloned but not saved.** Only the display `Snapshot` can be turned into JSON. That rules out one Raft feature, throwing away old log entries once a saved copy of the state exists. Phase 2 does without it: a restarted replica rebuilds its airport by re-applying its log from the start. Section 5 lists this as out of scope.
4. **The existing scenario format is a gift.** `Scenario.commands` is a flat, ordered list that already includes every `AdvanceTime`. It can be used unchanged as the script a client sends to the replicas, and `scenario::run` gives the exact outputs and final fingerprint a correct replicated run must also produce. Ticket E2 builds on this.
5. **The `host` crate is currently a pass-through.** It turns a scenario into a result with no replicas involved. Keep `run_scenario` exactly as it is: it becomes the reference answer.
6. **The `raft` crate is empty apart from a comment**, so nothing needs undoing.

### What changed in the second revision

The first version of this spec treated the host as the finished product: one program, with replicas as objects inside it. The aim is now different.

- **The system is built as if for production.** Every participant is a **node**: a replica, or a controller that owns the nodes below it. Each node is written so that it could run alone on its own machine or in its own container, talking to the others only by messages.
- **The host is one way of running the nodes, not the system itself.** It is the simulated runtime: all the nodes in one program, on a fake network, with a fake clock and fake disks. Compiled natively it does the bulk seeded test runs. Compiled to WebAssembly it is the live demo in the browser. The nodes it runs are the same code a real deployment would run.
- **"Node" no longer means "a participant in Raft".** That is now a **Raft member**, the agreement logic inside a replica. A node is any participant in the system's protocol.
- **The client of Phase 2 is now a controller.** The first version had a "client" that existed only in the host. The thing that numbers commands, finds the leader and resends is really the airport's own controller, which the design proposal introduced in Phase 3. It is built now, as the `LocalController`, because it is the node that owns the three replicas.

What this changed in the tickets: A3 is new; B1 to B6 say "member" where they said "node"; C2 and C3 are restated for nodes of any kind; D1 moves out of the `host` crate; D3 becomes the `LocalController`; D4 (the scenario driver) is new; H6 is new.

### Where the code stands (after commit `4c378ed` and this revision)

| Ticket | State |
|---|---|
| A1, A2 | Done. `det` holds `Rng`, the fingerprint and the identifier types (plus `Ms`). It also holds the values a Raft member exchanges with its replica (`Durable`, `Input`, `Effect`, `Message`). |
| A3 | The `node` crate and the `Node` trait exist, as signatures only. The system's `Message` type does not. |
| B1 | Mostly done. `raft::Member<C>` can be created, stepped and turned back into its `Durable`, with its invariants checked after every step. `Status` is still a placeholder with one variant. |
| B2 | Started. The term rule, vote granting and vote counting are written. `start_election`, `become_leader` and `send_refusal` are `todo!()`. `RequestVote` does not yet carry the candidate's last log position and term, and a grant does not compare logs. |
| B3 to B6 | Not started. `AppendEntries` and `AppendReply` exist as message shapes only. There is no commit position yet. |
| C1 | `Clock`, `EventQueue` and `Seeds` are done and tested. The loop itself (`Host::step`) is `todo!()`. |
| C2 | `host::Host<N>` exists as a skeleton: fields, signatures, and the steps written as comments. |
| Everything else | Not started. |

Two things were removed from the code in this revision, because they belonged to the old picture. The `raft::Node` trait with its `BrowserNode` implementation became the one concrete `raft::Member`: there must not be a browser version and a production version of the agreement logic, or the demo demonstrates nothing. The `Host` trait with `ChildHost` and `BrowserHost` (hosts nested inside hosts, each with its own queue) became the one flat `Host`: grouping is now the controllers' job, and a controller is a node, not a piece of the simulator.

## 3. Phase 2 on one page

**The goal.** Three copies of one airport, owned by one controller, that stay identical while the network loses messages and copies crash. All four are nodes that could each run on their own machine. In Phase 2 they run inside the host.

**The words used in this document.** Several of these are new since the design proposal, and "node" has changed meaning since the first version of this spec.

| Word | Meaning |
|---|---|
| **Node** | One participant in the system's protocol: a replica or a controller. It has an address (`NodeId`), receives messages and wakes, and sends messages. It could run on its own machine. |
| **Replica** | A node that holds one copy of an airport: the airport, a Raft member with its log, and the bookkeeping that joins them. |
| **Controller** | A node that owns one or more nodes below it. It is their only link to the rest of the system. |
| **LocalController** | The controller of one airport. It owns that airport's three replicas, and turns commands from above into numbered requests to them. |
| **AreaController** | A controller that owns several LocalControllers, for example the airports around one city. Phase 4. Named here only so that nothing in Phase 2 rules it out. |
| **Owns** | Knows the addresses of, is the parent of, and speaks for. It never means "holds in memory": a controller reaches the nodes it owns only by sending messages. |
| **Airport group** | The three replicas of one airport. Their Raft members form one Raft group. |
| **Raft member** | The agreement logic inside one replica. It holds a log and takes part in elections. It knows nothing about airports or controllers. Called "node" in the first version of this spec. |
| **Log** | A member's numbered list of entries. Positions start at 1. |
| **Entry** | One item in a log: a command, plus the election term in which a leader first stored it. |
| **Election term** | A counter that goes up every time any member starts an election. Raft calls this a "term". Every Raft message carries one. |
| **Leader, follower, candidate** | The three roles a member can be in. At most one leader per election term. |
| **Majority** | More than half of the members of a group. With three, two. |
| **Committed** | An entry is committed once it is guaranteed never to be lost or replaced. Only committed entries are applied to the airport. |
| **Client** | Whatever sends requests to an airport group and waits for replies. In this system that is the group's LocalController. |
| **Runtime** | Whatever runs a node: it delivers its messages, wakes it, tells it the time and keeps its durable state. |
| **Host** | The simulated runtime. One program that runs every node, and plays the network, the disks and the clock. It runs natively for bulk tests and as WebAssembly for the demo. |
| **Driver** | A stand-in node, in the host only, that sits where the LocalController's parent would be. It plays a scenario's commands and collects the answers. |
| **Checker** | Code in the host that inspects everything after every step and fails the run if a rule is broken. |
| **Seed** | The one number that all randomness in a run comes from. Same seed, same run. |

**The picture.** Solid arrows are ownership. Dotted lines are Raft messages between the members. Every box inside the host is a node, and every line is a message on the fake network.

```mermaid
flowchart TD
    subgraph Host["The host: fake network, fake disks, clock, faults, checkers"]
        Driver["Driver: plays a scenario's commands"]
        LC["LocalController: numbers, sends, resends"]
        subgraph R1["Replica 1"]
            N1["Raft member"] --> A1["Airport"]
        end
        subgraph R2["Replica 2"]
            N2["Raft member"] --> A2["Airport"]
        end
        subgraph R3["Replica 3"]
            N3["Raft member"] --> A3["Airport"]
        end
        Driver --> LC
        LC --> R1
        LC --> R2
        LC --> R3
    end
    N1 <-.-> N2
    N2 <-.-> N3
    N1 <-.-> N3
```

**The same nodes, two runtimes.**

| | The host (built in Phase 2) | A real runtime (optional ticket H6) |
|---|---|---|
| Where nodes live | All in one program | One per process, machine or container |
| Network | An event queue with seeded delays and faults | Sockets |
| Clock | A number the host advances | The machine's clock |
| Disk | A struct the host keeps | A file |
| Used for | Seeded tests, and the browser demo | Showing the nodes really are separable |

The rule that keeps both possible: the crates that hold the nodes (`det`, `airport`, `raft`, `node`) never depend on `host`.

**What gets built, in five pieces.**

| Piece | Lives in | What it is |
|---|---|---|
| A. Groundwork | `crates/det`, `crates/node` | Shared tools, identifier types, and the `Node` trait. |
| B. Raft | `crates/raft` | Elections, log copying, commitment. Pure logic. |
| C. The host | `crates/host` | Event loop, fake network, fake disk, fault injection, trace. |
| D. Replica and controller | `crates/node` (the driver in `crates/host`) | The replica, which joins Raft to the airport and filters duplicates, and the LocalController that owns three of them. |
| E. Checkers | `crates/host` | The rules that must never break, and proof that the checkers work. |

Pieces F (bulk runs and tooling) and G (the web view) are plumbing. Piece H is optional extras.

Each crate depends only on those before it: `det`, `airport`, `raft`, `node`, `host`, then `cli` and `wasm`.

## 4. Decisions already made in this spec

Each of these could have gone another way. They are fixed here so the tickets fit together. Change any of them if you disagree, but change it before starting.

| Decision | Choice | Why |
|---|---|---|
| What a node may touch | Only its own state. Inputs come in through `step`, and everything it wants done goes out as effects. No clock, network, disk, threads or references to other nodes. | This is what lets one node run in the host and in a process of its own without a line changed. |
| One implementation of each node | There is no browser version and production version of anything in `raft` or `node`. Only the runtime differs. | If the demo ran different logic from production, it would show nothing about production. |
| How the host arranges nodes | Flat: one map of nodes, one queue, one clock. The tree of who owns whom lives in the nodes' own settings. | It is how a real network looks: separate machines with addresses. A controller cannot hold a queue for nodes on other machines. |
| Who talks to whom | Replicas of a group send Raft messages straight to each other. Everything else goes up or down the tree: a replica hears from the outside only through its LocalController. | Raft traffic through a controller would make it a bottleneck and a single point of failure. |
| How a node learns the time | Every input to a node carries the current time. The node says when it next needs waking. The same goes for a Raft member inside a replica. | The host is an event loop, so "wake me at time T" costs one event. A regular tick would cost events when nothing is happening. |
| How Raft gets randomness | Each replica is given its own seeded generator when it is created, and passes it to its member. | Keeps the node pure, and the runtime stays in charge of the seed. |
| How the fake disk works | A node keeps everything that must survive a crash in one struct. On a crash the host keeps that struct and throws the rest of the node away. | It makes "what survives a crash" impossible to get wrong by accident, and costs nothing at run time. Disks that fail come in Phase 5. |
| When durable state is saved | The messages a node sends in one `step` leave only after its durable state at the end of that step is saved. In the host saving is instant, so this is free. | It is the promise Raft relies on (a vote is on disk before the reply is sent), stated so that a real runtime knows what it must do. |
| What Raft stores | Raft is generic over the command type. It never looks inside a command. | Keeps `raft` free of airports and of nodes, and lets the same code replicate controllers in Phase 5. |
| Where duplicates are filtered | In the replica, when an entry is applied. Not in Raft, not in the airport. | As the design proposal says. It is the only point where all three replicas are guaranteed to make the same decision. |
| Airport time and host time | Separate in Phase 2. Airport time is just a number inside `AdvanceTime` commands. Host time drives message delays and election timers. | A six-hour scenario can be replicated in a few simulated minutes. Tying the two clocks together is added to the LocalController in Phase 3. |
| How many clients | One: the LocalController, sending the scenario's commands in order with one request outstanding at a time. | With one client, the agreed log must equal the scenario, which gives an exact reference answer. |
| Which nodes can fail | Replicas crash and restart, and any link with a replica at one end can lose, delay or repeat messages. The LocalController and the driver do not crash, and the link between them is perfect. | Phase 2 is about the replicas agreeing. A controller that survives its own crash, and a lossy hop above it, need a parent that retries. That is Phase 4 and 5. |
| Group size | Three, but nothing in `raft` may assume three. | Tests should also run with one member and five. |

## 5. Out of scope for Phase 2

- **Discarding old log entries.** Logs grow for the whole run. (Raft calls the feature that fixes this "log compaction" or "snapshots".)
- **A real runtime.** Phase 2 builds only the host. The nodes are written so that a runtime with real sockets, a real clock and real files could run them unchanged, and ticket H6 is there for anyone who wants to prove it, but nothing in Phase 2 depends on it.
- **Controllers above the LocalController.** The AreaController is Phase 4. The driver stands in for it.
- **A controller that crashes**, and a replicated controller. Phase 5.
- **Changing which replicas are in the group** while it runs.
- **Disks that lose or corrupt data.** The fake disk is perfect. Phase 5.
- **Reading the airport's state through Raft.** The controller only sends commands. The web view looks at replicas directly through the host.
- **Tying airport time to host time**, the speed dial, and injecting events from a person. Phase 3.
- **Performance work** such as sending several entries per message as a tuned batch, or sending the next message before the previous one is acknowledged. Do the simplest correct thing.

## 6. Suggested order

The pieces are grouped by subject in section 7, but that is not the best order to build them in. This order gets something visibly running early and adds one hard idea at a time.

1. **A1, A2, A3.** Groundwork.
2. **B1, C1, D1 (first cut), C2.** An empty member, a replica that does nothing but run it, the event loop, and a perfect network. Three replicas that exchange nothing yet.
3. **B2, C6, E1 (the first rule only).** Elections. You can now watch a leader get elected in the trace, and a checker confirms there is never more than one.
4. **B3, B4, B5.** Logs are copied, repaired and committed.
5. **D1 (the rest), D3, D4.** An airport behind each member, a LocalController above the three replicas, and a driver playing a scenario at it. On a perfect network the final fingerprint should match Phase 1.
6. **E1 (the rest), E2.** The full set of checkers. Everything should pass on a perfect network.
7. **C3, B6.** Crashes and restarts.
8. **C4, C5.** Lost, late and repeated messages, split networks, and a seeded schedule of faults. This is where the bugs appear.
9. **D2.** Duplicate filtering. The checkers from step 6 will already be failing without it.
10. **E3, E4.** Progress after faults clear, and proof that the checkers can catch real mistakes.
11. **F1 to F3, G1 to G2.** Bulk runs, automation and the web view.

## 7. Tickets

### A. Groundwork

#### A1. Move the shared tools into their own crate
*Fine to delegate · Needs: nothing · Small*

**Build.** A new crate (suggested name `det`, for "deterministic") holding `Rng` and the fingerprint code, both moved unchanged from `airport`. `airport` re-exports them so nothing else has to change.

**Done when.** All existing tests pass untouched, including the web test that compares the native and WebAssembly builds, and `raft` can use `Rng` without depending on `airport`.

#### A2. Identifier types
*Fine to delegate · Needs: nothing · Small*

**Build.** Small wrapper types so that different kinds of number cannot be mixed up: a node identifier, a client identifier, an election term, and a log position. A node identifier is the address of any node, replica or controller. A Raft member has no identifier of its own: it goes by the identifier of the replica it is in. All fixed-width, none `usize`, all deriving `Hash`, `Ord` and the serde traits.

**Done when.** They exist and it is a compile error to pass a log position where an election term is expected.

#### A3. The `Node` trait and the `node` crate
*By hand · Needs: A2 · Small*

**Build.** A new crate, `node`, that will hold every participant of the system. In this ticket it holds only the interface they share and the one message type they exchange. A suggested shape, signatures only:

```rust
pub enum Input<M> {
    Wake,                                   // a time you asked to be woken at has passed
    Message { from: NodeId, message: M },
}

pub enum Effect<M> {
    Send { to: NodeId, message: M },
}

pub trait Node {
    type Message;
    fn id(&self) -> NodeId;
    fn step(&mut self, now: Ms, input: Input<Self::Message>) -> Vec<Effect<Self::Message>>;
    fn next_wake(&self) -> Option<Ms>;      // when the runtime should send the next Wake
}

/// Everything one node can say to another. Grows as the tickets land.
pub enum Message {
    Raft(det::objects::Message<Request>),   // replica to replica, same group (D1)
    Request(Request),                       // controller to one of its replicas (D2)
    Reply(ClientReply),                     // replica to its controller (D2)
    Submit(Command),                        // parent to controller: "do this next" (D3)
    Outputs(Vec<Output>),                   // controller to parent: the answer, in order (D3)
}
```

**Rules.**
- A node is written as if it ran alone on its own machine. It never reads a clock, opens a connection, touches a disk, starts a thread, or holds a reference to another node. Everything comes in through `step` and goes out as effects.
- A controller knows the nodes it owns only by their `NodeId`s, given to it when it is created. It never constructs them, holds them or calls them.
- `step` must be harmless for a `Wake` that arrives early or late, and `next_wake` is a pure read.
- `node` depends on `det`, `airport` and `raft`. Neither it nor anything it depends on may depend on `host`. The host is one runtime for nodes, and a node must not be able to tell which runtime it is in.
- There is one implementation of each node. Nothing in `node` or `raft` is compiled differently for the browser.

**Done when.** A toy node written in a test (for example one that answers every message with the same message) implements the trait, and a check in `just check` fails if `det`, `airport`, `raft` or `node` gains a dependency on `host`.

### B. Raft

Everything in this piece lives in `crates/raft`, is pure logic, and is tested without the host by a few lines of test code that hand messages from one member to another.

A **member** is the agreement logic of one replica. It is not a node: it has no address of its own, and it is the replica around it that implements `Node`, takes messages off the network and hands the Raft ones in. `raft` knows nothing of nodes, controllers or airports, only of the other members of its group.

One principle applies to every ticket here. **The network may deliver any message late, twice, out of order, or not at all.** Every handler must do the right thing in all four cases. A reply is just another message: there is no guarantee it arrives, and no guarantee it is the reply to the thing you sent most recently.

#### B1. The member's skeleton
*By hand · Needs: A1, A2 · Small*

**Build.** The member type and its interface, with no behaviour yet. It is one concrete type, `raft::Member<C>`, not a trait: the same code runs in every runtime. A suggested shape, signatures only:

```rust
pub enum Payload<C> { Noop, Command(C) }
pub struct Entry<C> { pub term: Term, pub payload: Payload<C> }

/// Everything that survives a crash. Nothing else does.
pub struct Durable<C> {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    pub log: Vec<Entry<C>>,
}

pub enum Input<C> {
    Wake,                                        // a time you asked to be woken at has passed
    Message { from: NodeId, message: Message<C> },
    Propose(C),                                  // the replica wants this command stored
}

pub enum Effect<C> {
    Send { to: NodeId, message: Message<C> },
    Committed { index: Index, entry: Entry<C> }, // in order, each position once per life of the member
    NotLeader { hint: Option<NodeId> },          // answer to a Propose this member cannot accept
}

impl<C: Clone + Eq> Member<C> {
    pub fn new(id: NodeId, peers: Vec<NodeId>, config: Config, durable: Durable<C>, rng: Rng, now: Ms) -> Self;
    pub fn step(&mut self, now: Ms, input: Input<C>) -> Vec<Effect<C>>;
    pub fn next_wake(&self) -> Option<Ms>;       // when the member next needs a Wake
    pub fn into_durable(self) -> Durable<C>;     // what is left after a crash
    pub fn status(&self) -> Status;              // role, term, positions: for checkers and the web view
    pub fn log(&self) -> &[Entry<C>];            // read-only, for checkers
}
```

**Rules.**
- A member starts as a follower in whatever election term its `Durable` says, which is 0 for a brand new member.
- Inside the member, split state clearly into what is in `Durable` and what is not. Role, the commit position, the list of who voted, and what a leader knows about its followers are all lost on a crash.
- A `Wake` that arrives early or late must be harmless. The member checks the time itself.

**Done when.** A member can be created, stepped with any input without panicking, and turned back into its `Durable`.

#### B2. Elections
*By hand · Needs: B1 · Medium*

**Build.** The two election messages (a request for a vote, and the reply) and the role changes between follower, candidate and leader.

**Rules.**
- **Timing.** A follower or candidate that goes a full election timeout without hearing from a leader or granting a vote starts an election. The timeout is chosen at random each time it is reset, between one and two times a base value from `Config`.
- **Starting an election.** Add one to the election term, vote for yourself, and ask every other member for its vote. The request states your term and the term and position of the last entry in your log.
- **Higher term seen.** Any message, of any kind, carrying a higher election term than your own makes you adopt that term, forget who you voted for, and become a follower, before you handle the message.
- **Lower term seen.** A request carrying a lower election term than your own is refused, and the refusal states your term.
- **Granting a vote.** Grant it only if you have not already voted for someone else in this election term, and the candidate's log is at least as up to date as yours. "Up to date" compares the term of the last entry first, and the length of the log second.
- **Winning.** A candidate that holds votes from a majority, counting its own, becomes leader for that election term and immediately tells everyone.
- **Losing.** A candidate that hears from a leader whose term is at least its own becomes a follower.

**Tests.**
- Three members, messages handed over by hand: one times out, the other two vote, it becomes leader.
- Two candidates in the same term: at most one wins.
- A member that has voted refuses a second candidate in the same term, and grants the same candidate again if asked twice.
- A candidate with a shorter log is refused by a member with a longer one.
- A cluster of one elects itself.
- A vote reply from an earlier election term changes nothing.

#### B3. Copying the log
*By hand · Needs: B2 · Medium*

**Build.** The message a leader uses to send entries to a follower, its reply, and `Propose`.

**Rules.**
- **Heartbeat.** A leader sends every follower a message at a regular interval from `Config`, whether or not there is anything new to send. This is what stops followers starting elections.
- **Proposing.** A leader given a `Propose` adds the command to the end of its own log, stamped with its election term. Any other member answers `NotLeader`, with the leader it last heard from if it knows one.
- **What the message says.** The leader's term; the position and term of the entry just before the ones being sent; the entries; and the leader's commit position.
- **The consistency check.** A follower accepts the message only if its own log has an entry at that "just before" position with that same term. Otherwise it refuses.
- **What the leader tracks.** For each follower: the next position to send, and the highest position it knows that follower holds.

**Tests.**
- A proposal to the leader appears in all three logs after the messages are handed over.
- A proposal to a follower returns `NotLeader` with the right hint.
- With no proposals, followers never time out while heartbeats are delivered.
- Entries sent to a follower with an empty log are accepted from position 1.

#### B4. Repairing a log that has diverged
*By hand · Needs: B3 · Medium*

**Build.** What happens when a follower's log does not match the leader's.

**Rules.**
- **On refusal**, the leader steps back and tries again from an earlier position, until it finds a point where the two logs agree.
- **On acceptance**, if the follower already has an entry at a position being sent but with a different term, that entry and everything after it in the follower's log is deleted, and the leader's entries take their place.
- **A leader never deletes or overwrites anything in its own log.**

**Tests.** Build these situations by hand by constructing each member's `Durable` directly.
- A follower missing the last few entries catches up.
- A follower with extra entries from an old leader that were never committed has them removed and replaced.
- A follower whose log is entirely different after position 3 is repaired from position 4.
- The six follower logs in Figure 7 of the Raft paper all end up identical to the leader's.

#### B5. Commitment, and handing entries over to be applied
*By hand · Needs: B4 · Medium*

**Build.** The rule for when an entry is safe forever, and the `Committed` effect.

**Rules.**
- **Leader.** A position is committed once a majority of members, counting the leader, hold the entry at that position, **and** that entry is from the leader's own current election term. Everything before a committed position is committed too.
- **A new leader's first act** is to add an empty entry (`Payload::Noop`) from its own term to its log. Without this, entries left over from earlier terms could sit uncommitted forever when no new commands are arriving.
- **Follower.** A follower learns the commit position from the leader's messages. It never moves its own commit position past the last entry that the message it just accepted vouches for.
- **Handing over.** Each time the commit position moves forward, the member emits one `Committed` effect per newly committed position, in order, with no gaps and no repeats.
- **The commit position never moves backwards.**

**Tests.**
- An entry held by two of three members is committed; an entry held only by the leader is not.
- A leader isolated from both followers accepts proposals but commits none of them.
- The situation in Figure 8 of the Raft paper: an entry from an old term that is on a majority of members is not committed until an entry from the current term is.
- After a new leader is elected with no new proposals, leftover entries become committed because of the empty entry.

#### B6. Surviving a crash
*By hand · Needs: B5 · Small*

**Build.** Nothing new, if B1's split was done properly. This ticket is the check that it was.

**Rules.**
- A member rebuilt from its `Durable` is a follower, with a commit position of 0, that remembers its election term, who it voted for in that term, and its whole log.
- Because the commit position restarts at 0, a restarted member emits `Committed` for every position again, from 1, as it relearns the commit position from the leader.

**Tests.**
- A member that voted, crashed and restarted refuses to vote for a different candidate in the same term.
- A leader that crashes and restarts does not believe it is still leader.
- A restarted follower re-emits `Committed` for positions 1 onwards, in order.

### C. The host

Everything in this piece lives in `crates/host`. The host is the simulated runtime: it runs any set of nodes, and knows them only through the `Node` trait. It is generic plumbing in one sense, but its fault injection is half of what makes the project "like TigerBeetle", so most of it is marked by hand.

The host is flat. It keeps every node side by side in one map, with one queue and one clock, however the nodes are arranged among themselves. There are no hosts inside hosts.

#### C1. The event loop and the clock
*By hand · Needs: A1 · Small*

**Build.** The to-do list at the centre of the host: events ordered by simulated time, and a loop that takes the earliest, moves the clock to it, and handles it.

**Rules.**
- Two events at the same simulated time are handled in the order they were added. This must be explicit, not an accident of the data structure.
- Simulated time never goes backwards.
- All randomness comes from generators derived from one seed. Use a separate generator for each purpose (network, faults, each life of each node), each derived from the seed, so that changing the fault settings does not change the message delays.
- There is one clock and one queue for the whole run, and only the host moves the clock.

**Done when.** A test schedules events out of order and sees them handled in time order, with ties in insertion order, and the same seed gives the same sequence twice.

#### C2. A perfect network, and the nodes on it
*By hand · Needs: C1, A3 · Small*

**Build.** The host is handed a set of nodes when it is created. When a node emits `Send`, the host schedules a delivery event a fixed short time later. When a node's `next_wake` changes, the host schedules a `Wake`.

**Rules.**
- The host is generic over `Node`. It does not know what a replica or a controller is, and can be tested with toy nodes.
- Nodes never hold references to each other or to the host. Messages are values that move through the event list.
- Any node may send to any other node of the run. A `Send` to an address the host was never given is a bug in the sender, and fails loudly.
- A `Wake` scheduled for a time the node no longer cares about is delivered anyway and ignored by the node.

**Done when.** Two toy nodes pass a message back and forth in a test. Then, with B2 and the first cut of D1 in place, three replicas on the perfect network elect exactly one leader and keep it.

#### C3. The fake disk, crashes and restarts
*By hand · Needs: C2, B6, D1 · Small*

**Build.** Two host operations. Crash: take the node's durable state, keep it, and discard the node. Restart: build a new node from the kept state, with a new generator derived from the seed. `Node` gains whatever the host needs to do this for any node (suggested: an associated `Durable` type, `into_durable(self)`, and a function given to the host that builds a node from an id, its durable state, a generator and the time).

**Rules.**
- Messages addressed to a crashed node are dropped when they would have been delivered.
- A `Wake` for a crashed node is dropped. One left over from before the crash that fires after a restart is harmless.
- A replica's durable state is its member's `Durable` and nothing else. The airport and the duplicate table are discarded on a crash and rebuilt from the log (D1).
- The messages a node emits in a `step` are released only after its durable state at the end of that step is saved. In the host this is automatic, because the state is read out of the node at the moment of the crash. The rule is written down for the sake of a real runtime.
- In Phase 2 only replicas are crashed. The LocalController and the driver run for the whole run.

**Done when.** Crashing the leader on the perfect network produces a new leader within a few election timeouts, and restarting the old one brings it back as a follower with a matching log.

#### C4. An unreliable network
*By hand · Needs: C2 · Medium*

**Build.** Replace the perfect network's fixed delay with seeded misbehaviour, controlled by settings:

| Fault | Behaviour |
|---|---|
| Delay | Each message is delivered after a random time between a minimum and a maximum. Messages overtaking each other falls out of this for free. |
| Loss | Each message is dropped with a given probability. |
| Duplication | Each message is delivered a second time, after a different delay, with a given probability. |
| Partition | A set of blocked links. A message whose link is blocked at the moment of delivery is dropped. Links are one-directional, so "A can reach B but B cannot reach A" is possible. |

**Rules.**
- Faults apply to every link with a replica at one end: replica to replica, and replica to LocalController in both directions.
- The link between the driver and the LocalController keeps the perfect network's fixed delay and is never faulty (section 4, "Which nodes can fail").
- The network decides by address only. It never looks at what kind of node is at either end, or inside a message.

**Done when.** Unit tests show each fault doing what the table says, and the same seed produces the same pattern of faults twice.

#### C5. A seeded schedule of faults
*By hand · Needs: C3, C4 · Medium*

**Build.** A generator that turns a seed and a difficulty level into a list of timed fault events: crash this replica, restart it, block these links, unblock them. The host adds them to its event list at the start of a run.

**Rules.**
- Three levels, in the spirit of the TigerBeetle demo:

| Level | Network | Faults |
|---|---|---|
| Calm | Short delays, nothing lost. | None. |
| Rough | Longer delays, some loss and duplication. | One replica at a time crashed or cut off, so a majority can always talk to each other and to the LocalController. |
| Hostile | Delays longer than the election timeout, heavy loss and duplication. | Anything, including a majority down at once, one-directional partitions, and faults in quick succession. |

- Every schedule ends with a **heal**: all replicas restarted, all links unblocked, network set to calm. The run then continues for a fixed stretch of simulated time. Ticket E3 depends on this.
- At the hostile level the group will often make no progress for long periods. That is expected. Only safety is being tested there until the heal.

**Done when.** A seed and a level always produce the same schedule, and a run at each level reaches its heal.

#### C6. The trace
*Fine to delegate · Needs: C2 · Small*

**Build.** A list of everything the host did, in order, as a serde-serialisable enum: message sent, delivered, dropped or duplicated; a member's role or election term changed; position committed; entry applied with the fingerprint afterwards; node crashed or restarted; links blocked or unblocked; the LocalController sent or received something; the driver submitted a command or received its outputs.

**Rules.**
- Recording the trace must not change the run. A run with the trace switched off has the same fingerprints as one with it on.
- The trace is the host's. Nodes do not write to it: the host records what it sees them do (the messages they send, and what their read-only accessors show after each step).

**Done when.** A run can print its trace as readable lines, and as JSON for the web view in piece G.

### D. Replica and controller

The replica and the LocalController live in `crates/node`, because they are the system itself and must not depend on the simulator. Both implement `Node`. The driver (D4) is the exception: it exists only to feed the simulator, so it lives in `crates/host`.

#### D1. The replica
*By hand · Needs: A3, B1 (first cut); B5 (the rest) · Medium*

**Build.** A node that owns one Raft member and one `Airport`. It hands Raft messages and wakes to the member, sends on the messages the member wants sent, and for every `Committed` effect it applies the entry to the airport.

**First cut.** A replica that only carries Raft messages between its member and the runtime is enough for C2 and can be written as soon as B1 exists. Applying entries waits for B5.

**Rules.**
- A replica is created with its own `NodeId`, the ids of the other replicas in its group, and the id of the LocalController that owns it. Its member's peers are those other replicas.
- A Raft message is accepted only from a replica of its own group. Anything else on the network reaches it only from its LocalController.
- The airport is only ever changed by applying committed entries, in order.
- An empty entry (`Payload::Noop`) is skipped.
- After applying each entry the replica records the position and the airport's fingerprint, and offers both through a read-only accessor for the checkers and the trace.
- A restarted replica starts with a fresh `Airport::new` and rebuilds it as its member re-emits `Committed` from position 1.
- The replica answers its controller only if its member is the leader at the moment the entry is applied.
- The replica's `next_wake` is its member's.

**Done when.** On the perfect network with commands proposed straight to the leader, all three replicas report the same fingerprint at every position.

#### D2. Filtering duplicate commands
*By hand · Needs: D1, D3 · Medium*

**Build.** The wrapper that travels through Raft, and the table that filters repeats.

```rust
pub struct Request { pub client: ClientId, pub seq: u64, pub command: Command }

pub enum ClientReply {
    Done { seq: u64, outputs: Vec<Output> },
    NotLeader { hint: Option<NodeId> },
}
```

**Rules.**
- Raft replicates `Request` values. The airport still only ever sees the `Command` inside.
- The client is the LocalController. Its `ClientId` is given to it when it is created.
- Each replica keeps a table with one row per client: the highest `seq` applied for that client, and the outputs that application produced.
- When a committed `Request` is about to be applied:
  - If its `seq` is one more than the client's row, apply it, and store the outputs in the row.
  - If its `seq` equals the client's row, do **not** apply it. If this replica is the leader, send the stored outputs again.
  - If its `seq` is lower than the client's row, ignore it.
  - A `seq` more than one above the row cannot happen with a correct client. Treat it as a bug and fail loudly.
- The table is changed only by applying committed entries, so it is identical on all replicas at the same position. Include it in the replica's fingerprint.
- The table is lost in a crash and rebuilt along with the airport.

**Tests.**
- The same `Request` committed at two positions changes the airport once, and the second produces the same reply as the first.
- A retried `RequestLanding` does not produce `Rejected: DuplicateFlight` and does not change the rejected count.
- Two replicas given the same log, including repeats, have the same table and the same fingerprint.

#### D3. The LocalController
*By hand · Needs: D1 · Medium*

**Build.** The node that owns the three replicas of one airport. It takes commands from its parent, one `Submit` at a time, gets each one agreed by the group, and sends the outputs back up. This is the "client" of the first version of this spec, and the controller of the design proposal's Phase 3, built as a node.

**Rules.**
- A LocalController is created with its own `NodeId`, its `ClientId`, the id of its parent, and the ids of the replicas it owns. It never constructs, holds or calls a replica: it only sends to those ids.
- Commands submitted by the parent are queued and handled in the order they arrived.
- The controller has at most one request outstanding. It does not send request *n+1* until it has a `Done` reply for request *n*.
- Each new command gets the next `seq`. A resend uses the same `seq`.
- The controller sends to the replica it believes is leader. On `NotLeader` it follows the hint if there is one, and otherwise tries another replica.
- If no reply arrives within a timeout, it resends, to a different replica. The timeout is its `next_wake`.
- A reply whose `seq` is not the one outstanding is ignored.
- For each `Done`, the controller sends the outputs to its parent, in the order the commands were submitted, each exactly once.
- The controller's requests and the replicas' replies travel over the same fake network as everything else, so they can be lost, delayed and duplicated too.
- The controller keeps nothing durable and does not crash in Phase 2.
- It does not send `AdvanceTime` by itself in Phase 2. Time arrives as commands from above, like everything else (section 4, "Airport time and host time").

**Done when.** A test that hands messages between one LocalController and three replicas by hand, with no host, gets a command agreed and its outputs sent to the parent, and survives a `NotLeader` and a lost reply.

#### D4. The scenario driver
*Fine to delegate · Needs: D3, C2 · Small*

**Build.** A node, in `crates/host`, that stands where the LocalController's parent will stand in Phase 4. It is given a `Scenario`, sends its commands to the LocalController as `Submit` messages in order, and collects the `Outputs` that come back. Together with it, the function that builds a whole Phase 2 run: one driver, one LocalController and three replicas, each with its generator from the seed, handed to the host.

**Rules.**
- The driver is the only node that is not part of the system. It lives in `host` so that `node` holds nothing a real deployment would not ship.
- The host holds nodes of three kinds in one map. How it does so (an enum, or trait objects) is up to you, but checkers must still be able to reach a replica's read-only accessors.
- The link between the driver and the LocalController is perfect (C4), so the driver needs no retries.

**Done when.** On the perfect network, the driver plays a whole demo scenario and the outputs it collected, in order, equal `scenario::run`'s outputs for the same scenario.

### E. Checkers

The checkers run inside the host after every event. They can see inside every replica through its read-only accessors, and through those its member's `status()` and `log()`. A failure stops the run and reports the seed, the simulated time, and which rule broke.

Checkers belong to the simulator, not to the system: no node knows they exist, and they are the one place allowed to look at several nodes at once.

Write the checkers with the same discipline as `tests/invariants.rs` in the airport crate: they may share types with the code they check, but none of its logic.

#### E1. Raft's safety rules
*By hand · Needs: B5, C2 · Medium*

**Build.** One checker per rule. The first five are the five guarantees stated in the Raft paper.

| Rule | What must hold |
|---|---|
| One leader per term | No two members are ever leader in the same election term. This holds across the whole run, not just at one instant. |
| Leaders only append | While a member is leader, it never removes or changes an entry in its own log. |
| Logs match | If two members' logs have an entry with the same term at the same position, their logs are identical from position 1 up to that position. |
| Leaders are complete | Once an entry is committed, every member that becomes leader in a later term has it in its log. |
| Same entry at each position | No two members ever emit `Committed` for the same position with different entries. |
| Terms only rise | A member's election term never decreases, including across a crash and restart. |
| One vote per term | A member's recorded vote never changes within an election term, including across a crash and restart. |
| Commit only rises | Within one life of a member, its commit position never decreases, and a committed entry is never removed from any log. |

**How.** Several of these need the checker to remember things for the whole run: who has been leader in each term, and what has been committed at each position.

**Done when.** A long calm run passes all eight, and each has a unit test that feeds the checker a deliberately broken state and sees it fail.

#### E2. The airport's rules, end to end
*By hand · Needs: D1, D3, D4, E1 · Medium*

**Build.** Three checkers above the Raft level.

| Rule | What must hold |
|---|---|
| Replicas agree | Any two replicas that have applied the same position report the same fingerprint at it. |
| Exactly once | For each client and `seq`, each replica changes its airport at most once per life, however many times the request appears in the log. |
| Same answer as one airport | At the end of the run, every live replica's airport fingerprint, and the driver's collected outputs in order, equal what `scenario::run` gives for the same scenario with no replication at all. |

**Why the third rule is so strong.** With one LocalController sending one request at a time, the agreed log has to contain the scenario's commands in the scenario's order. So a correct replicated run, however badly the network behaved, must end in exactly the state Phase 1 computes. And since Phase 1's own tests show that state obeys the aviation rules, there is no need to re-check separation or closures here.

**Done when.** All three pass on a calm run, and the third is the final line of every run's report.

#### E3. Progress once the faults stop
*By hand · Needs: C5, D4 · Small*

**Build.** A check at the end of every run, after the heal at the end of the fault schedule.

**Rules.** Within a fixed stretch of simulated time after the heal:
- there is a leader;
- the driver has received the outputs of every command in the scenario;
- all three replicas have applied the same last position.

**Done when.** A run that would otherwise sit forever with no leader is reported as a failure with its seed, not left to hang.

#### E4. Proof that the checkers can catch mistakes
*By hand · Needs: everything above · Medium*

**Build.** A set of switches, compiled only for tests, each of which breaks the implementation in one specific way. The switches sit in `raft` and `node`, where the mistakes would be made. They are the one permitted exception to "nothing is compiled differently", and they must not exist in a normal build. For each switch, a test runs seeds until a checker fails, and fails itself if none does within a fixed number of seeds.

**Why.** A simulator that has never caught a bug has not been shown to work. This is the only way to know that "ten thousand seeds passed" means something.

> **Spoiler.** The list below names well-known mistakes in Raft implementations. If you want to find them yourself, do not read it until your own implementation passes E1 to E3 at the hostile level.

| Switch | The mistake |
|---|---|
| 1 | Grant a vote without comparing logs. |
| 2 | Forget who you voted for when restarting. |
| 3 | Forget the election term when restarting. |
| 4 | Commit an entry from an earlier term as soon as a majority holds it. |
| 5 | On accepting entries, always cut the log off after the last one sent, even when nothing conflicted. |
| 6 | As a follower, set the commit position to whatever the leader says, even beyond what the accepted message covered. |
| 7 | Count two copies of the same vote reply as two votes. |
| 8 | Let a reply from an earlier election term update what the leader believes a follower holds. |
| 9 | Skip the duplicate table and apply every committed request. |
| 10 | As the LocalController, give a resend a new `seq`. |

**Done when.** Every switch is caught, and the test output names which checker caught it and at which seed. Any switch that is never caught means a checker or the fault schedule is too weak, and that is the thing to fix.

### F. Bulk runs and tooling

#### F1. Running many seeds from the command line
*Fine to delegate · Needs: C5, E3 · Small*

**Build.** A new `airport-sim` subcommand that runs a range of seeds at a chosen fault level against a short scenario, stops at the first failure, and prints the seed, the rule that broke, and the exact command to replay it with the trace on.

**Done when.** `just` has a recipe for a quick run (a few hundred seeds) and an overnight run (as many as fit), and a deliberately broken build is reported with a seed that replays to the same failure.

#### F2. Run it twice
*Fine to delegate · Needs: F1, C6 · Small*

**Build.** A test that runs the same seed twice natively and once as WebAssembly, and compares the three traces entry by entry. This extends the existing comparison in `web/src/sim.test.ts`.

**Done when.** It passes for several seeds at each fault level.

#### F3. Automation
*Fine to delegate · Needs: F1, F2 · Small*

**Build.** The automatic checks on every commit run a few hundred seeds at each level, plus the E4 tests.

**Done when.** A commit that breaks agreement fails the automatic checks with a seed in the output.

### G. The web view

#### G1. Expose a replicated run to the browser
*Fine to delegate · Needs: C6 · Small*

**Build.** A function in the `wasm` crate that takes a seed and a fault level and returns the trace as JSON, alongside the existing `runScenario`. The `wasm` crate stays a thin wrapper around `host`: the nodes it runs are the ordinary ones from `node`.

#### G2. The replica view
*Fine to delegate · Needs: G1 · Medium*

**Build.** A second page beside the runway timeline. Three boxes, one per replica, showing role, election term, last log position, commit position and state fingerprint. Above them one box for the LocalController, showing its outstanding request and which replica it believes is leader. Messages drawn moving between all four. Crashed replicas greyed out, blocked links shown cut. A seed box, a fault level selector, and play, pause and step controls that walk through the trace.

**Done when.** Typing in a seed printed by an overnight failure shows that failure happening.

### H. Optional extras

All by hand. None is needed to finish Phase 2. Each is a self-contained thing to learn.

| Ticket | What | Why it is interesting |
|---|---|---|
| H1 | Faster log repair: on a refusal, the follower says enough for the leader to skip back a whole term at once. | Section 5.3 of the Raft paper sketches it. Matters when logs are long. |
| H2 | A trial election before the real one, so a member that was cut off cannot disrupt a healthy leader when it returns. | Known as "pre-vote". The hostile level will show you the problem it solves. |
| H3 | A leader that steps down if it cannot reach a majority for a while. | Stops a cut-off leader accepting proposals it can never commit. |
| H4 | Discarding old log entries, with a saved copy of the airport and the duplicate table. | Needs `Airport` to be saveable, and a new message for followers that are too far behind. The largest of these. |
| H5 | A second client (a second controller with its own `ClientId`) sending its own commands at the same time. | Exercises one table row per client. The "same answer as one airport" rule has to be restated: replay the agreed log through a fresh airport and compare. |
| H6 | A real runtime: a second binary that runs one node per process, with messages over TCP, the machine's clock, and `Durable` in a file. Four processes make one airport: three replicas and their LocalController. | It is the proof that the nodes are what they claim to be. It needs a way to turn `Message` into bytes, and a crate that is exempt from the determinism ban, since it must read a real clock. Nothing in `node` or `raft` should need to change, and if something does, that is the finding. |

## 8. Finished when

Phase 2 is finished when all of these are true.

1. Every ticket in pieces A to F is done.
2. `det`, `airport`, `raft` and `node` build with no dependency on `host`, and nothing in them is compiled differently for the browser.
3. An overnight run at the hostile level completes with no checker failing.
4. Every switch in ticket E4 is caught.
5. The same seed gives the same trace natively, twice, and as WebAssembly.
6. A failing seed, produced on purpose with an E4 switch, can be typed into the web view and watched.

## 9. Reading

- [In Search of an Understandable Consensus Algorithm (the Raft paper, extended version)](https://raft.github.io/raft.pdf). Figure 2 is the one-page summary that tickets B2 to B5 restate. Figures 7 and 8 are used as tests in B4 and B5.
- [Diego Ongaro's dissertation](https://github.com/ongardie/dissertation). Chapter 6 covers clients and duplicate filtering, which is ticket D2.
- [Students' Guide to Raft](https://thesquareplanet.com/blog/students-guide-to-raft/). A catalogue of the mistakes students make. It is a spoiler in the same way as ticket E4.
- [tikv/raft-rs](https://github.com/tikv/raft-rs). Not used in this project, but written in the same "pure logic, no clock or network" shape, so useful to read when stuck on structure.
- [We Put a Distributed Database In the Browser (TigerBeetle)](https://tigerbeetle.com/blog/2023-07-11-we-put-a-distributed-database-in-the-browser/). The fault levels in ticket C5 follow its three levels.

## 10. Appendix: shape of `step` and `next_wake` (B1 to B6)

This is an overview of how the pieces in tickets B1 to B6 fit together inside one Raft member. It adds no new requirements. Where it picks between options, the choice is marked as a suggestion.

### 10.1 Roles

```mermaid
stateDiagram-v2
    [*] --> Follower: new / restart (from Durable)
    Follower --> Candidate: Wake past election deadline
    Candidate --> Candidate: Wake past election deadline (new term, new election)
    Candidate --> Leader: majority of votes in this term
    Candidate --> Follower: AppendEntries with term >= mine
    Leader --> Follower: any message with a higher term
    Candidate --> Follower: any message with a higher term
```

A crash is not a transition. The runtime throws the whole replica away and builds a new one, member included, from `Durable`, which always starts as a follower (B6).

### 10.2 State, split by what survives a crash

| `Durable` (kept) | Volatile (lost) |
|---|---|
| `term`, `voted_for`, `log` | `role`, `commit`, `leader_hint` |
| | `election_deadline: Option<Ms>` (follower, candidate) |
| | `votes: set of NodeId` (candidate) |
| | `heartbeat_deadline: Option<Ms>`, `next[peer]`, `matched[peer]` (leader) |

### 10.3 Shape of `step`

```text
step(now, input):
    effects = []

    # 1. Time first. A Wake that is early or late is harmless because we only
    #    compare against our own deadlines. Checking on every input, not just
    #    Wake, means a late Wake can never hide an expired timer.
    if role is Leader and now >= heartbeat_deadline:
        send_append_to_all(); heartbeat_deadline = now + cfg.heartbeat
    if role is not Leader and now >= election_deadline:
        start_election(now)

    # 2. Then the input.
    match input:
        Wake                  => nothing more to do
        Propose(cmd)          => if Leader: append Entry{term, Command(cmd)}; send_append_to_all(); advance_commit()
                                 else:      effects.push(NotLeader{hint: leader_hint})
        Message{from, msg}    => handle_message(from, msg)

    return effects
```

`advance_commit()` after a `Propose` matters for a cluster of one: the leader alone is a majority, so no reply will ever arrive to trigger it.

### 10.4 `handle_message`

Every message first goes through the term rule (B2), whatever its type:

```text
handle_message(from, msg):
    if msg.term > term:
        term = msg.term; voted_for = None; become Follower; leader_hint = None
    if msg.term < term:
        # stale. Requests get a refusal that carries our term; replies are dropped.
        if msg is a request: send refusal(term) to from
        return
    dispatch on msg type...
```

| Message | Handling |
|---|---|
| **RequestVote** | Grant if `voted_for` is `None` or `from`, **and** the candidate's last (term, index) is at least ours. On grant, set `voted_for = from` and reset the election deadline. On refusal, do **not** reset it. |
| **VoteReply** | Ignore unless we are a candidate in this term. Add the voter to `votes` (a set, so duplicates do nothing). On a majority, `become_leader`. |
| **AppendEntries** | Same term as ours, so there is a leader: become Follower if candidate, set `leader_hint = from`, reset the election deadline. Run the consistency check (B3). Refuse on mismatch. Otherwise delete any conflicting suffix, append the new entries (B4), set `commit = min(leader_commit, index of last entry this message vouches for)`, and emit `Committed` for each newly covered position (B5). The reply states how far the logs are known to match. |
| **AppendReply** | Ignore unless we are Leader in this term. On success: `matched[from] = max(matched[from], reply.matched)`, `next[from] = matched[from] + 1`, then `advance_commit()`. On refusal: `next[from] = max(1, next[from] - 1)` and resend to that peer. |

Reasons for the choices above:
- Replies carry what they matched rather than "the last thing I sent", and `matched` only moves up, so duplicated, late or reordered replies cannot move it backwards.
- Only a valid message from a current-term leader, or a granted vote, resets the election deadline. A refused vote or a stale-term message must not, or a member that is being ignored could stay quiet forever.

### 10.5 Transitions

```text
start_election(now):
    term += 1; voted_for = self; role = Candidate; votes = {self}
    election_deadline = now + random(base, 2*base)       # fresh random draw every time
    send RequestVote(term, last_index, last_term) to each peer
    if votes is already a majority: become_leader(now)    # cluster of one

become_leader(now):
    role = Leader; election_deadline = None
    for each peer: next[peer] = log.len() + 1; matched[peer] = 0
    append Entry{term, Noop}                              # B5: lets older entries commit
    send_append_to_all(); heartbeat_deadline = now + cfg.heartbeat
    advance_commit()                                      # cluster of one

advance_commit():
    # highest N above commit where a majority hold N (leader counts itself)
    # AND log[N].term == current term; never move backwards
    for each newly committed position i: effects.push(Committed{i, log[i]})
```

### 10.6 `next_wake`

It is a pure read of the stored deadlines. It must not draw from the `Rng`, so two calls in a row give the same answer.

```text
next_wake():
    match role:
        Leader                 => heartbeat_deadline     # a leader has no election timer
        Follower | Candidate   => election_deadline
```

In practice this is always `Some`. `None` is only needed for a leader with no peers, which has nothing to send. A replica reports its member's `next_wake` as its own, and the host re-reads it after every `step` and schedules a new `Wake` if it changed (C2). Older `Wake`s still fire and are ignored by the check at the top of `step`.

### 10.7 Setting deadlines in `new`

`Member::new(.., now)` takes a time so it can set `election_deadline = now + random(base, 2*base)` straight away. After a restart this makes the member wait a full timeout before it may stand for election, which gives a surviving leader time to reach it first (suggestion, not required by B6).

## 11. Appendix: how the nodes and the host fit together (A3, C2, D1 to D4)

Like section 10, this adds no requirements.

### 11.1 Layers

```text
host        the simulated runtime: clock, queue, fake network, fake disk, faults, checkers, trace, driver
  |  drives, through the Node trait only
node        Replica, LocalController        (each one a Node; could each be a process)
  |  a replica contains
raft        Member<Request>                 (inputs in, effects out; no address of its own)
airport     Airport                         (apply a command, get outputs)
det         Rng, fingerprint, NodeId, Term, Index, Ms
```

Dependencies only point downwards. `cli` and `wasm` sit on top of `host`.

### 11.2 One command, start to finish

```text
Driver          --Submit(cmd)-->                 LocalController
LocalController --Request{client, seq, cmd}-->   Replica 2 (believed leader)
Replica 2:      member.step(Propose(request))    -> Send AppendEntries to replicas 1 and 3
Replica 2       --Raft(AppendEntries)-->         Replicas 1, 3
Replicas 1, 3   --Raft(AppendReply)-->           Replica 2
Replica 2:      member emits Committed           -> duplicate table, then Airport::apply
Replica 2       --Reply(Done{seq, outputs})-->   LocalController
LocalController --Outputs(outputs)-->            Driver
```

Each arrow is one `Effect::Send` that the host turns into a `Deliver` event. Replicas 1 and 3 apply the same entry when they learn it is committed, and stay silent because they are not the leader.

### 11.3 Inside `Host::step`

```text
step():
    pop the earliest event, or return false
    clock.advance_to(its time)
    Wake { node }                 => step_node(node, Input::Wake)
    Deliver { from, to, message } => step_node(to, Input::Message { from, message })
    (C3, C5 add: Crash, Restart, Block, Unblock, Heal)
    run the checkers

step_node(id, input):
    if the node is crashed: drop the input
    for each Send { to, message } the node returns:
        let the network decide: drop, or push Deliver at now + delay (and perhaps a second copy)
    if node.next_wake() changed: push a Wake
```

### 11.4 What a real runtime would do instead

The same `step_node`, for one node, driven by a socket and a timer instead of a queue: read a message or wait until `next_wake`, call `step` with the machine's time, save the node's durable state, then write each `Send` to the socket for its address. Nothing in the node changes. That is the whole point of the split, and ticket H6 is the test of it.
