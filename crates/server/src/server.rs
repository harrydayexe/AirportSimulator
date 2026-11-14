//! The loop that owns the replica, and the client API in front of it.
//!
//! One thread owns the [`ProdHost`] and the [`Replica`]. Everything else
//! (peer connections, client connections) runs on other threads and talks to
//! it through one queue of [`Inbound`] values, so the deterministic parts
//! are only ever touched from one place, in one order.
//!
//! That thread is the "owner" the [`Host`] trait describes, with the real
//! clock: it steps the host for every event that has come due, does what the
//! reports say (send this, apply that), then sleeps until the next event is
//! due or something arrives.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use det::objects::Message;
use det::{ClientId, Config, Ms, NodeId, Rng, Role, Term};
use host::airport::{AirportConfig, Output, Snapshot};
use host::event::{Event, Report};
use host::host::Host;
use host::prodhost::ProdHost;
use serde::Serialize;
use serde_json::json;

use crate::config::{AirportChoice, Options, split_address};
use crate::http::{self, Response};
use crate::replica::{Outcome, Replica, Request};
use crate::storage::FileStorage;
use crate::transport::Transport;
use crate::wire::cluster_fingerprint;

/// Things waiting for the loop before senders are made to wait.
const INBOX: usize = 4_096;
/// The most arrivals taken in before due events are looked at again.
const BATCH: usize = 1_024;
/// How long the loop sleeps when the host has nothing scheduled.
const IDLE: Duration = Duration::from_secs(1);
/// How long a client waits for its command to be committed.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a status question waits for the loop.
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);

/// Something for the loop to deal with.
pub enum Inbound {
    /// A Raft message from another replica, already checked.
    Peer {
        /// The replica that sent it.
        from: NodeId,
        /// The message.
        message: Message<Request>,
    },
    /// A client's command.
    Client {
        /// The command and its numbering.
        request: Request,
        /// Where to send the answer.
        reply: Sender<ClientReply>,
    },
    /// Someone wants to know how the replica is.
    Status {
        /// Where to send the answer.
        reply: Sender<Status>,
    },
    /// Someone wants to see the airport.
    Snapshot {
        /// Where to send the answer.
        reply: Sender<Snapshot>,
    },
}

/// The answer to a client's command.
#[derive(Debug, PartialEq, Eq)]
pub enum ClientReply {
    /// The command was committed and applied.
    Done {
        /// The command's number.
        seq: u64,
        /// What the airport decided.
        outputs: Vec<Output>,
    },
    /// This replica is not the leader.
    NotLeader {
        /// The leader, if this replica knows it.
        leader: Option<NodeId>,
    },
    /// Leadership changed while the command was waiting. It may or may not
    /// have been stored; sending it again with the same number is safe.
    Retry,
    /// A later command from this client has already been applied.
    Stale {
        /// The client's latest applied command.
        latest: u64,
    },
    /// The client skipped a number.
    Gap {
        /// The number its next command must carry.
        expected: u64,
    },
}

/// How a replica is, for `/status` and the probes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    /// This replica.
    pub id: NodeId,
    /// `leader`, `follower` or `candidate`.
    pub role: &'static str,
    /// Its election term.
    pub term: Term,
    /// The leader of that term, if known.
    pub leader: Option<NodeId>,
    /// The last position in its log.
    pub last_index: u64,
    /// The last position it knows to be committed.
    pub commit: u64,
    /// The last position applied to its airport.
    pub applied: u64,
    /// The fingerprint of its airport and duplicate table, as 16
    /// hexadecimal digits. Equal on every replica at the same `applied`.
    pub fingerprint: String,
    /// Every member of the cluster.
    pub members: Vec<NodeId>,
}

/// Reads the airport every replica starts from.
fn load_airport(choice: &AirportChoice) -> Result<AirportConfig, String> {
    match choice {
        AirportChoice::Demo => Ok(AirportConfig::demo()),
        AirportChoice::Single => Ok(AirportConfig::single_runway()),
        AirportChoice::File(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            serde_json::from_str(&text)
                .map_err(|e| format!("{}: not an airport: {e}", path.display()))
        }
    }
}

/// A seed for election timeouts: different on every start of every replica.
fn seed(id: NodeId) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    (u64::from(id.0) << 48) ^ (u64::from(std::process::id()) << 32) ^ u64::from(nanos)
}

/// The loop's state. See the module documentation.
struct Server {
    /// The node, its queue and its disk.
    host: ProdHost<Request, FileStorage<Request>>,
    /// The airport behind the node.
    replica: Replica,
    /// The way to the other replicas.
    transport: Transport,
    /// This replica.
    id: NodeId,
    /// Every member of the cluster.
    members: Vec<NodeId>,
    /// When the process started; host time is milliseconds since then.
    started: Instant,
    /// Clients waiting for a command this replica proposed as leader.
    waiting: BTreeMap<(ClientId, u64), Sender<ClientReply>>,
    /// The term those commands were proposed in.
    waiting_term: Term,
    /// The role, term and leader last written to the log output.
    announced: (Role, Term, Option<NodeId>),
}

impl Server {
    /// The host's time: whole milliseconds since the process started.
    fn now(&self) -> Ms {
        Ms(u64::try_from(self.started.elapsed().as_millis()).expect("uptime fits in u64"))
    }

    fn status(&self) -> Status {
        let node = self.host.node();
        Status {
            id: self.id,
            role: match node.role() {
                Role::Leader => "leader",
                Role::Follower => "follower",
                Role::Candidate => "candidate",
            },
            term: node.term(),
            leader: node.leader(),
            last_index: node.last_index().0,
            commit: node.commit().0,
            applied: self.replica.applied().0,
            fingerprint: format!("{:016x}", self.replica.fingerprint()),
            members: self.members.clone(),
        }
    }

    /// Takes in one arrival. Nothing is stepped here; messages and commands
    /// join the host's queue at `now`.
    fn accept(&mut self, now: Ms, inbound: Inbound) {
        // A reply that cannot be delivered means the asker gave up waiting.
        match inbound {
            Inbound::Peer { from, message } => {
                self.host.inject(
                    now,
                    Event::Deliver {
                        from,
                        to: self.id,
                        message,
                    },
                );
            }
            Inbound::Status { reply } => drop(reply.send(self.status())),
            Inbound::Snapshot { reply } => drop(reply.send(self.replica.snapshot())),
            Inbound::Client { request, reply } => {
                let node = self.host.node();
                if node.role() != Role::Leader {
                    drop(reply.send(ClientReply::NotLeader {
                        leader: node.leader(),
                    }));
                } else if let Some(outcome) = self.replica.known(request.client, request.seq) {
                    drop(reply.send(answer(request.seq, outcome)));
                } else {
                    self.waiting_term = node.term();
                    // A resend takes over from the first try, which is
                    // already in the log.
                    let first_try = self
                        .waiting
                        .insert((request.client, request.seq), reply)
                        .is_none();
                    if first_try {
                        self.host.inject(
                            now,
                            Event::Propose {
                                to: self.id,
                                command: request,
                            },
                        );
                    }
                }
            }
        }
    }

    /// Does what the host's reports say.
    fn handle(&mut self, reports: Vec<Report<Request>>) {
        for report in reports {
            match report {
                Report::Outbound { to, message, .. } => self.transport.send(to, message),
                Report::Committed { index, entry, .. } => {
                    if let Some((client, seq, outcome)) = self.replica.apply(index, &entry)
                        && let Some(reply) = self.waiting.remove(&(client, seq))
                    {
                        drop(reply.send(answer(seq, outcome)));
                    }
                }
                // The waiting clients are told below, once the steps are done.
                Report::NotLeader { .. } => {}
            }
        }
    }

    /// After a round of steps: says so if the role changed, and lets go of
    /// clients whose commands this replica can no longer vouch for.
    fn settle(&mut self) {
        let node = self.host.node();
        let state = (node.role(), node.term(), node.leader());
        if state != self.announced {
            let (role, term, leader) = state;
            let leader = leader.map_or_else(|| "nobody".to_owned(), |l| l.to_string());
            eprintln!(
                "replica {}: {role:?} in term {term}, led by {leader}",
                self.id
            );
            self.announced = state;
        }
        let (role, term, leader) = state;
        if !self.waiting.is_empty() && (role != Role::Leader || term != self.waiting_term) {
            for (_, reply) in std::mem::take(&mut self.waiting) {
                drop(reply.send(if role == Role::Leader {
                    ClientReply::Retry
                } else {
                    ClientReply::NotLeader { leader }
                }));
            }
        }
    }

    /// Runs for ever.
    fn run(mut self, inbound: &Receiver<Inbound>) -> String {
        loop {
            let now = self.now();
            while let Some(at) = self.host.next_event_at().filter(|at| *at <= now) {
                let reports = self.host.step(at);
                self.handle(reports);
            }
            self.settle();

            let wait = self.host.next_event_at().map_or(IDLE, |at| {
                Duration::from_millis(at.0.saturating_sub(self.now().0))
            });
            match inbound.recv_timeout(wait) {
                Ok(first) => {
                    let now = self.now();
                    self.accept(now, first);
                    for arrival in inbound.try_iter().take(BATCH) {
                        self.accept(now, arrival);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return "every listener has stopped".to_owned();
                }
            }
        }
    }
}

/// What a client is told about an applied request.
fn answer(seq: u64, outcome: Outcome) -> ClientReply {
    match outcome {
        Outcome::Done(outputs) => ClientReply::Done { seq, outputs },
        Outcome::Stale { latest } => ClientReply::Stale { latest },
        Outcome::Gap { expected } => ClientReply::Gap { expected },
    }
}

/// Asks the loop a question and waits up to `timeout` for the answer.
fn ask<T>(
    inbound: &SyncSender<Inbound>,
    timeout: Duration,
    question: impl FnOnce(Sender<T>) -> Inbound,
) -> Option<T> {
    let (reply, answer) = channel();
    // `try_send`: if the loop is stuck, say so instead of joining the queue.
    inbound.try_send(question(reply)).ok()?;
    answer.recv_timeout(timeout).ok()
}

/// Answers one HTTP request. `client_urls` is where each replica's client
/// port is, for pointing clients at the leader.
fn respond(
    request: &http::Request,
    inbound: &SyncSender<Inbound>,
    client_urls: &BTreeMap<NodeId, String>,
) -> Response {
    let status = || ask(inbound, STATUS_TIMEOUT, |reply| Inbound::Status { reply });
    let stuck = || Response::error(503, "the replica is not responding");
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") => {
            status().map_or_else(stuck, |_| Response::json(200, &json!({ "status": "ok" })))
        }
        ("GET", "/readyz") => status().map_or_else(stuck, |status| {
            let code = if status.leader.is_some() { 200 } else { 503 };
            Response::json(code, &json!(status))
        }),
        ("GET", "/status") => {
            status().map_or_else(stuck, |status| Response::json(200, &json!(status)))
        }
        ("GET", "/snapshot") => ask(inbound, STATUS_TIMEOUT, |reply| Inbound::Snapshot { reply })
            .map_or_else(stuck, |snapshot| Response::json(200, &json!(snapshot))),
        ("POST", "/commands") => {
            let request: Request = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(e) => return Response::error(400, &format!("not a request: {e}")),
            };
            match ask(inbound, COMMAND_TIMEOUT, |reply| Inbound::Client {
                request,
                reply,
            }) {
                Some(ClientReply::Done { seq, outputs }) => {
                    Response::json(200, &json!({ "seq": seq, "outputs": outputs }))
                }
                Some(ClientReply::NotLeader {
                    leader: Some(leader),
                }) => Response {
                    location: client_urls
                        .get(&leader)
                        .map(|url| format!("{url}/commands")),
                    ..Response::json(307, &json!({ "error": "not_leader", "leader": leader }))
                },
                Some(ClientReply::NotLeader { leader: None }) => {
                    Response::json(503, &json!({ "error": "no_leader" }))
                }
                Some(ClientReply::Retry) => Response::json(503, &json!({ "error": "retry" })),
                Some(ClientReply::Stale { latest }) => {
                    Response::json(409, &json!({ "error": "stale_seq", "latest": latest }))
                }
                Some(ClientReply::Gap { expected }) => {
                    Response::json(409, &json!({ "error": "seq_gap", "expected": expected }))
                }
                None => Response::json(504, &json!({ "error": "timeout" })),
            }
        }
        (_, "/healthz" | "/readyz" | "/status" | "/snapshot" | "/commands") => {
            Response::error(405, "method not allowed")
        }
        _ => Response::error(404, "not found"),
    }
}

/// Starts the replica `options` describes and runs it for ever.
///
/// # Errors
/// If the replica cannot start: a bad airport, an unreadable or damaged data
/// directory, or an address that cannot be listened on. Once started it only
/// returns if its listeners die.
pub fn run(options: &Options) -> Result<(), String> {
    let id = options.id;
    let airport = load_airport(&options.airport)?;
    let replica = Replica::new(airport.clone()).map_err(|e| format!("invalid airport: {e}"))?;
    let members: Vec<NodeId> = options.peers.keys().copied().collect();
    let cluster = cluster_fingerprint(&members, &airport);

    let storage = FileStorage::open(&options.data_dir).map_err(|e| e.to_string())?;
    let config = Config {
        heartbeat_interval: Ms(options.heartbeat_ms),
        base_election_timeout: Ms(options.election_ms),
    };
    let started = Instant::now();
    let host = ProdHost::new(id, &members, config, storage, Rng::new(seed(id)), Ms(0))
        .map_err(|e| format!("{}: {e}", options.data_dir.display()))?;

    let listen = |address: &str| {
        TcpListener::bind(address).map_err(|e| format!("cannot listen on {address}: {e}"))
    };
    let (peer_listener, client_listener) = (
        listen(&options.listen_peer)?,
        listen(&options.listen_client)?,
    );

    let (inbox, inbound) = sync_channel(INBOX);
    let others: BTreeMap<NodeId, String> = options
        .peers
        .iter()
        .filter(|(peer, _)| **peer != id)
        .map(|(p, a)| (*p, a.clone()))
        .collect();
    let transport = Transport::start(id, cluster, &others, peer_listener, inbox.clone())
        .map_err(|e| format!("cannot start the peer transport: {e}"))?;

    // Every replica is assumed to serve clients on the same port number.
    let client_port = split_address(&options.listen_client).map_or(8080, |(_, port)| port);
    let client_urls: BTreeMap<NodeId, String> = options
        .peers
        .iter()
        .filter_map(|(peer, address)| {
            let (host, _) = split_address(address)?;
            Some((*peer, format!("http://{host}:{client_port}")))
        })
        .collect();
    http::serve(client_listener, move |request| {
        respond(&request, &inbox, &client_urls)
    })
    .map_err(|e| format!("cannot start the client port: {e}"))?;

    let node = host.node();
    eprintln!(
        "replica {id} of {members:?}: term {}, {} log entries, peers on {}, clients on {}, cluster {cluster:016x}",
        node.term(),
        node.last_index(),
        options.listen_peer,
        options.listen_client,
    );
    let announced = (node.role(), node.term(), node.leader());
    let server = Server {
        host,
        replica,
        transport,
        id,
        members,
        started,
        waiting: BTreeMap::new(),
        waiting_term: Term(0),
        announced,
    };
    Err(server.run(&inbound))
}
