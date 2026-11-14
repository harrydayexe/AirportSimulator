//! The real network between replicas: TCP, one line of JSON per message.
//!
//! Raft expects messages to be lost, so this never waits and never retries.
//! Each peer has a small queue and a thread that writes it to a connection,
//! reconnecting when needed; a message for a peer that cannot be reached, or
//! whose queue is full, is dropped and Raft sends another later.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufReader, Write as _};
use std::net::{TcpListener, TcpStream, ToSocketAddrs as _};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use det::NodeId;
use det::objects::Message;

use crate::replica::Request;
use crate::server::Inbound;
use crate::wire::{Frame, read_line, validate};

/// Messages waiting for one peer before new ones are dropped.
const OUTBOX: usize = 1_024;
/// How long to wait for a connection to open.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a write may block before the connection is given up.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to leave a peer alone after failing to reach it.
const RETRY_AFTER: Duration = Duration::from_millis(250);
/// A connection that says nothing for this long is closed. A leader's
/// heartbeats keep the ones in use well inside it.
const READ_TIMEOUT: Duration = Duration::from_mins(1);

/// The sending half: one queue per peer.
pub struct Transport {
    /// The queue for each peer's writer thread.
    outboxes: BTreeMap<NodeId, SyncSender<String>>,
}

impl Transport {
    /// Starts accepting replicas on `listener` and starts a writer for each
    /// of `peers` (the other members and their addresses). Messages that
    /// arrive are checked and sent to `inbound`.
    ///
    /// # Errors
    /// If a thread cannot be started.
    pub fn start(
        id: NodeId,
        cluster: u64,
        peers: &BTreeMap<NodeId, String>,
        listener: TcpListener,
        inbound: SyncSender<Inbound>,
    ) -> io::Result<Self> {
        let hello = Frame::Hello { from: id, cluster }.to_line();
        let mut outboxes = BTreeMap::new();
        for (&peer, address) in peers {
            let (queue, messages) = sync_channel(OUTBOX);
            let (address, hello) = (address.clone(), hello.clone());
            thread::Builder::new()
                .name(format!("to-{peer}"))
                .spawn(move || write_to(peer, &address, &hello, &messages))?;
            outboxes.insert(peer, queue);
        }
        let members: BTreeSet<NodeId> = peers.keys().copied().collect();
        thread::Builder::new()
            .name("accept-peers".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let (members, inbound) = (members.clone(), inbound.clone());
                    // A failure to start one reader only loses that connection.
                    let _ = thread::Builder::new()
                        .name("from-peer".to_owned())
                        .spawn(move || {
                            if let Err(why) = read_from(stream, cluster, &members, &inbound) {
                                eprintln!("peer connection closed: {why}");
                            }
                        });
                }
            })?;
        Ok(Self { outboxes })
    }

    /// Queues `message` for `to`. Dropped if the queue is full.
    pub fn send(&self, to: NodeId, message: Message<Request>) {
        let Some(queue) = self.outboxes.get(&to) else {
            return;
        };
        match queue.try_send(Frame::Message(message).to_line()) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
        }
    }
}

/// Opens a connection to `address` and says hello.
fn connect(address: &str, hello: &str) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no address found");
    // Looked up every time: a replica may come back at a new address.
    for candidate in address.to_socket_addrs()? {
        match TcpStream::connect_timeout(&candidate, CONNECT_TIMEOUT) {
            Ok(mut stream) => {
                stream.set_nodelay(true)?;
                stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
                stream.write_all(hello.as_bytes())?;
                return Ok(stream);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// One peer's writer: sends each queued line, connecting as needed.
fn write_to(peer: NodeId, address: &str, hello: &str, messages: &Receiver<String>) {
    let mut stream: Option<TcpStream> = None;
    let mut retry_at = Instant::now();
    // Say that a peer is unreachable once, not once per message.
    let mut reported = false;
    for line in messages {
        if stream.is_none() {
            if Instant::now() < retry_at {
                continue;
            }
            match connect(address, hello) {
                Ok(connected) => {
                    eprintln!("connected to replica {peer} at {address}");
                    stream = Some(connected);
                    reported = false;
                }
                Err(why) => {
                    if !reported {
                        eprintln!("cannot reach replica {peer} at {address}: {why}");
                        reported = true;
                    }
                    retry_at = Instant::now() + RETRY_AFTER;
                    continue;
                }
            }
        }
        if let Some(connected) = &mut stream
            && let Err(why) = connected.write_all(line.as_bytes())
        {
            eprintln!("lost connection to replica {peer}: {why}");
            stream = None;
            retry_at = Instant::now() + RETRY_AFTER;
        }
    }
}

/// One accepted connection: checks the hello, then passes each message on.
fn read_from(
    stream: TcpStream,
    cluster: u64,
    members: &BTreeSet<NodeId>,
    inbound: &SyncSender<Inbound>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|e| format!("socket: {e}"))?;
    let mut reader = BufReader::new(stream);
    let mut frame = || -> Result<Option<Frame>, String> {
        let Some(line) = read_line(&mut reader).map_err(|e| format!("read: {e}"))? else {
            return Ok(None);
        };
        serde_json::from_slice(&line)
            .map(Some)
            .map_err(|e| format!("not a frame: {e}"))
    };
    let from = match frame()? {
        Some(Frame::Hello {
            from,
            cluster: theirs,
        }) => {
            if !members.contains(&from) {
                return Err(format!("replica {from} is not in this cluster"));
            }
            if theirs != cluster {
                return Err(format!(
                    "replica {from} was started with different --peers or --airport"
                ));
            }
            from
        }
        _ => return Err("no hello".to_owned()),
    };
    while let Some(frame) = frame()? {
        let Frame::Message(message) = frame else {
            return Err(format!("replica {from} said hello twice"));
        };
        validate(&message).map_err(|why| format!("replica {from} sent a bad message: {why}"))?;
        if inbound.send(Inbound::Peer { from, message }).is_err() {
            break; // The server is stopping.
        }
    }
    Ok(())
}
