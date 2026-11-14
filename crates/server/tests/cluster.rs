//! Runs three real `airport-server` processes on this machine, talking over
//! real sockets and writing real files, and checks the group gives the same
//! answer as one airport with no replication at all, including across the
//! leader being killed and brought back.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use airport_server::http::request;
use host::airport::Output;
use host::airport::scenario::{self, Scenario};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(30);

/// A port nothing is listening on (for a moment, at least).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Cluster {
    dir: PathBuf,
    peers: String,
    /// `(peer port, client port)` of replicas 1 to 3.
    ports: Vec<(u16, u16)>,
    running: Vec<Option<Child>>,
}

impl Cluster {
    fn start(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("airport-cluster-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ports: Vec<_> = (0..3).map(|_| (free_port(), free_port())).collect();
        let peers = (1..=3)
            .map(|id| format!("{id}=127.0.0.1:{}", ports[id - 1].0))
            .collect::<Vec<_>>()
            .join(",");
        let mut cluster = Self {
            dir,
            peers,
            ports,
            running: vec![None, None, None],
        };
        for id in 1..=3 {
            cluster.launch(id);
        }
        cluster
    }

    fn launch(&mut self, id: usize) {
        let (peer, client) = self.ports[id - 1];
        let child = Command::new(env!("CARGO_BIN_EXE_airport-server"))
            .args(["--id", &id.to_string(), "--peers", &self.peers])
            .args(["--listen-peer", &format!("127.0.0.1:{peer}")])
            .args(["--listen-client", &format!("127.0.0.1:{client}")])
            .args(["--heartbeat-ms", "20", "--election-ms", "150"])
            .arg("--data-dir")
            .arg(self.dir.join(id.to_string()))
            .stderr(Stdio::null())
            .spawn()
            .expect("airport-server starts");
        self.running[id - 1] = Some(child);
    }

    /// Kills replica `id` outright, as a power cut would.
    fn kill(&mut self, id: usize) {
        let mut child = self.running[id - 1].take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }

    fn address(&self, id: usize) -> String {
        format!("127.0.0.1:{}", self.ports[id - 1].1)
    }

    fn get(&self, id: usize, path: &str) -> Option<Value> {
        let (status, body) = request(&self.address(id), "GET", path, "").ok()?;
        (status == 200).then(|| serde_json::from_str(&body).unwrap())
    }

    /// Waits until `ready` holds for what `/status` says on every running replica.
    fn wait_for(&self, what: &str, ready: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let statuses: Option<Vec<Value>> = (1..=3)
                .filter(|id| self.running[id - 1].is_some())
                .map(|id| self.get(id, "/status"))
                .collect();
            if let Some(statuses) = statuses
                && ready(&statuses)
            {
                return statuses;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            sleep(Duration::from_millis(20));
        }
    }

    /// The one leader the running replicas agree on.
    fn leader(&self) -> usize {
        let statuses = self.wait_for("a leader", |statuses| {
            let leader = &statuses[0]["leader"];
            leader.is_u64()
                && statuses.iter().all(|s| s["leader"] == *leader)
                && statuses.iter().filter(|s| s["role"] == "leader").count() == 1
        });
        usize::try_from(statuses[0]["leader"].as_u64().unwrap()).unwrap()
    }

    /// Sends command `seq` the way a client would: to whoever it thinks
    /// leads, following redirects and retrying until it has an answer.
    fn send(&self, at: &mut usize, seq: u64, command: &Value) -> Vec<Output> {
        let body = json!({ "client": 1, "seq": seq, "command": command }).to_string();
        let deadline = Instant::now() + TIMEOUT;
        loop {
            assert!(
                Instant::now() < deadline,
                "command {seq} was never answered"
            );
            let answer = request(&self.address(*at), "POST", "/commands", &body);
            let Ok((status, text)) = answer else {
                *at = *at % 3 + 1;
                continue;
            };
            let reply: Value = serde_json::from_str(&text).unwrap();
            match status {
                200 => {
                    assert_eq!(reply["seq"], seq);
                    return serde_json::from_value(reply["outputs"].clone()).unwrap();
                }
                307 => *at = usize::try_from(reply["leader"].as_u64().unwrap()).unwrap(),
                503 | 504 => {
                    *at = *at % 3 + 1;
                    sleep(Duration::from_millis(20));
                }
                _ => panic!("command {seq}: {status} {text}"),
            }
        }
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        for child in self.running.iter_mut().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn log_lines(dir: &Path, id: usize) -> usize {
    std::fs::read_to_string(dir.join(id.to_string()).join("log"))
        .unwrap()
        .lines()
        .count()
}

#[test]
fn three_processes_give_the_same_answer_as_one_airport() {
    let scenario: Scenario = scenario::demo(3, 1, 30, 30);
    let commands: Vec<Value> = scenario
        .commands
        .iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect();
    let commands = &commands[..400.min(commands.len())];
    let reference = scenario::run(&Scenario {
        airport: scenario.airport.clone(),
        commands: scenario.commands[..commands.len()].to_vec(),
    })
    .unwrap();

    let mut cluster = Cluster::start("answer");
    let first_leader = cluster.leader();
    let mut at = first_leader % 3 + 1; // start at a follower: it redirects
    let mut outputs = Vec::new();
    let mut seq = 0;
    let third = commands.len() / 3;

    for command in &commands[..third] {
        seq += 1;
        outputs.extend(cluster.send(&mut at, seq, command));
    }
    assert_eq!(at, first_leader, "the client found the leader");

    // A resend of the last command gets the same answer and changes nothing.
    let before = cluster.get(at, "/status").unwrap();
    let again = cluster.send(&mut at, seq, &commands[third - 1]);
    assert!(outputs.ends_with(&again));
    let after = cluster.get(at, "/status").unwrap();
    assert_eq!(before["fingerprint"], after["fingerprint"]);
    assert_eq!(
        before["applied"], after["applied"],
        "answered from the table"
    );
    // Skipping ahead or going back is refused.
    let post = |seq: u64| {
        let body = json!({ "client": 1, "seq": seq, "command": commands[0] }).to_string();
        request(&cluster.address(at), "POST", "/commands", &body).unwrap()
    };
    assert_eq!(post(seq + 5).0, 409);
    assert_eq!(post(1).0, 409);
    assert_eq!(
        request(&cluster.address(at), "POST", "/commands", "{")
            .unwrap()
            .0,
        400
    );
    assert_eq!(
        request(&cluster.address(at), "GET", "/nowhere", "")
            .unwrap()
            .0,
        404
    );

    // Kill the leader. The other two carry on.
    cluster.kill(first_leader);
    let second_leader = cluster.leader();
    assert_ne!(second_leader, first_leader);
    for command in &commands[third..2 * third] {
        seq += 1;
        outputs.extend(cluster.send(&mut at, seq, command));
    }

    // Bring it back: it catches up from its own files and the new leader.
    let on_disk = log_lines(&cluster.dir, first_leader);
    assert!(on_disk > third, "its log survived: {on_disk} entries");
    cluster.launch(first_leader);
    for command in &commands[2 * third..] {
        seq += 1;
        outputs.extend(cluster.send(&mut at, seq, command));
    }

    let statuses = cluster.wait_for("every replica to apply everything", |statuses| {
        let applied = &statuses[0]["applied"];
        statuses.len() == 3
            && statuses
                .iter()
                .all(|s| s["applied"] == *applied && s["commit"] == s["last_index"])
    });
    for status in &statuses {
        assert_eq!(
            status["fingerprint"], statuses[0]["fingerprint"],
            "replicas agree"
        );
    }
    // Same answer as one airport: every output, in order, and the end state.
    assert_eq!(outputs, reference.outputs);
    for id in 1..=3 {
        let snapshot = cluster.get(id, "/snapshot").unwrap();
        assert_eq!(
            snapshot,
            serde_json::to_value(&reference.end).unwrap(),
            "replica {id}"
        );
        assert_eq!(cluster.get(id, "/healthz").unwrap()["status"], "ok");
        assert!(cluster.get(id, "/readyz").is_some());
    }
}

#[test]
fn a_leader_alone_stays_alive_but_commits_nothing() {
    let mut cluster = Cluster::start("alone");
    let leader = cluster.leader();
    for id in (1..=3).filter(|id| *id != leader) {
        cluster.kill(id);
    }
    let before = cluster.get(leader, "/status").unwrap();
    let body = json!({
        "client": 1,
        "seq": 1,
        "command": { "type": "advance_time", "now_ms": 1000 }
    });
    let address = cluster.address(leader);
    let (status, text) = request(&address, "POST", "/commands", &body.to_string()).unwrap();
    assert_eq!(status, 504, "{text}");
    let after = cluster.get(leader, "/status").unwrap();
    assert_eq!(
        before["applied"], after["applied"],
        "stored, but never applied"
    );
    assert_eq!(after["commit"], before["commit"]);
    assert_eq!(cluster.get(leader, "/healthz").unwrap()["status"], "ok");
}

#[test]
fn replicas_started_with_different_airports_refuse_each_other() {
    let dir = std::env::temp_dir().join(format!("airport-cluster-mismatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ports: Vec<_> = (0..2).map(|_| (free_port(), free_port())).collect();
    let peers = format!("1=127.0.0.1:{},2=127.0.0.1:{}", ports[0].0, ports[1].0);
    let mut children: Vec<Child> = [(1, "demo"), (2, "single")]
        .into_iter()
        .map(|(id, airport)| {
            Command::new(env!("CARGO_BIN_EXE_airport-server"))
                .args([
                    "--id",
                    &id.to_string(),
                    "--peers",
                    &peers,
                    "--airport",
                    airport,
                ])
                .args(["--listen-peer", &format!("127.0.0.1:{}", ports[id - 1].0)])
                .args(["--listen-client", &format!("127.0.0.1:{}", ports[id - 1].1)])
                .args(["--heartbeat-ms", "20", "--election-ms", "100"])
                .arg("--data-dir")
                .arg(dir.join(id.to_string()))
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    // Plenty of election timeouts: with two members and no agreement on
    // what the cluster is, neither may ever lead.
    sleep(Duration::from_secs(2));
    for (_, client) in &ports {
        let (status, body) = request(&format!("127.0.0.1:{client}"), "GET", "/status", "").unwrap();
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(status, 200);
        assert_ne!(body["role"], "leader", "{body}");
        assert_eq!(body["leader"], Value::Null);
    }
    for child in &mut children {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
