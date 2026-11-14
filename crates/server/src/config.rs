//! Command-line options. Every option can also come from an environment
//! variable, which is how the container image is configured; an argument
//! wins over the environment.

use std::collections::BTreeMap;
use std::path::PathBuf;

use det::NodeId;

/// Printed for `--help` and after any mistake.
pub const USAGE: &str = "\
usage: airport-server [options]
       airport-server healthcheck [HOST:PORT]

Runs one replica of the airport group. Start one per machine, each with the
same --peers and --airport and its own --id and --data-dir.

options (environment variable):
  --id N                 this replica's number            (AIRPORT_ID)
  --id-from-hostname     use the number after the last `-` of the hostname,
                         plus one: `airport-0` is replica 1
                                                           (AIRPORT_ID_FROM_HOSTNAME=1)
  --peers LIST           every replica, this one included, as
                         1=host:port,2=host:port,3=host:port
                                                           (AIRPORT_PEERS)
  --listen-peer ADDR     where replicas reach this one     (AIRPORT_LISTEN_PEER)
                         default 0.0.0.0:7000
  --listen-client ADDR   where clients reach this one      (AIRPORT_LISTEN_CLIENT)
                         default 0.0.0.0:8080
  --data-dir PATH        where the log is kept             (AIRPORT_DATA_DIR)
                         default ./data
  --airport WHICH        `demo`, `single`, or a JSON file  (AIRPORT_CONFIG)
                         default demo
  --heartbeat-ms N       leader heartbeat interval         (AIRPORT_HEARTBEAT_MS)
                         default 100
  --election-ms N        base election timeout             (AIRPORT_ELECTION_MS)
                         default 1000

healthcheck asks a running replica (default 127.0.0.1:8080) whether it is
alive, and exits 0 if it is.
";

/// Which airport every replica starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AirportChoice {
    /// The three-runway demonstration airport.
    Demo,
    /// One runway.
    Single,
    /// An `AirportConfig` as JSON in this file.
    File(PathBuf),
}

/// Everything the command line decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// This replica.
    pub id: NodeId,
    /// Every replica, this one included, and where its peer port is.
    pub peers: BTreeMap<NodeId, String>,
    /// The address to accept other replicas on.
    pub listen_peer: String,
    /// The address to accept clients on.
    pub listen_client: String,
    /// The directory for durable state.
    pub data_dir: PathBuf,
    /// The airport to start from.
    pub airport: AirportChoice,
    /// The leader's heartbeat interval in milliseconds.
    pub heartbeat_ms: u64,
    /// The base election timeout in milliseconds.
    pub election_ms: u64,
}

/// The longest election timeout accepted: an hour.
const MAX_ELECTION_MS: u64 = 3_600_000;

/// Each option that takes a value, with its environment variable.
const VALUES: [(&str, &str); 8] = [
    ("--id", "AIRPORT_ID"),
    ("--peers", "AIRPORT_PEERS"),
    ("--listen-peer", "AIRPORT_LISTEN_PEER"),
    ("--listen-client", "AIRPORT_LISTEN_CLIENT"),
    ("--data-dir", "AIRPORT_DATA_DIR"),
    ("--airport", "AIRPORT_CONFIG"),
    ("--heartbeat-ms", "AIRPORT_HEARTBEAT_MS"),
    ("--election-ms", "AIRPORT_ELECTION_MS"),
];

/// `host` out of `host:port`, and the port.
#[must_use]
pub fn split_address(address: &str) -> Option<(&str, u16)> {
    let (host, port) = address.rsplit_once(':')?;
    (!host.is_empty()).then_some((host, port.parse().ok()?))
}

/// `1=a:7000,2=b:7000` as a map.
fn parse_peers(list: &str) -> Result<BTreeMap<NodeId, String>, String> {
    let mut peers = BTreeMap::new();
    for item in list
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let (id, address) = item
            .split_once('=')
            .ok_or_else(|| format!("--peers: `{item}` is not ID=HOST:PORT"))?;
        let id: u32 = id
            .parse()
            .map_err(|_| format!("--peers: `{id}` is not a number"))?;
        if split_address(address).is_none() {
            return Err(format!("--peers: `{address}` is not HOST:PORT"));
        }
        if peers.insert(NodeId(id), address.to_owned()).is_some() {
            return Err(format!("--peers: replica {id} is listed twice"));
        }
    }
    Ok(peers)
}

/// `airport-0` is replica 1.
fn id_from_hostname(hostname: Option<&str>) -> Result<u32, String> {
    let hostname = hostname.ok_or("--id-from-hostname: the hostname is not known")?;
    hostname
        .rsplit_once('-')
        .and_then(|(_, ordinal)| ordinal.parse::<u32>().ok())
        .and_then(|ordinal| ordinal.checked_add(1))
        .ok_or_else(|| format!("--id-from-hostname: `{hostname}` does not end in -NUMBER"))
}

/// Reads the options from `args` (without the program name), falling back
/// to `env` for anything not given. `hostname` is this machine's name.
///
/// # Errors
/// A message saying what is wrong.
pub fn parse(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
    hostname: Option<&str>,
) -> Result<Options, String> {
    let mut values: BTreeMap<&str, String> = VALUES
        .iter()
        .filter_map(|(name, variable)| Some((*name, env(variable).filter(|v| !v.is_empty())?)))
        .collect();
    let mut from_hostname =
        env("AIRPORT_ID_FROM_HOSTNAME").is_some_and(|v| v == "1" || v == "true");
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--id-from-hostname" {
            from_hostname = true;
        } else if let Some((name, _)) = VALUES.iter().find(|(name, _)| name == arg) {
            let value = args.next().ok_or_else(|| format!("{name} needs a value"))?;
            values.insert(name, value.clone());
        } else {
            return Err(format!("unexpected argument {arg}"));
        }
    }
    let number = |name: &str, default: u64| -> Result<u64, String> {
        values.get(name).map_or(Ok(default), |v| {
            v.parse()
                .map_err(|_| format!("{name} needs a whole number, not `{v}`"))
        })
    };
    let text =
        |name: &str, default: &str| values.get(name).map_or(default, String::as_str).to_owned();

    let id = match values.get("--id") {
        Some(id) => id
            .parse()
            .map_err(|_| format!("--id needs a whole number, not `{id}`"))?,
        None if from_hostname => id_from_hostname(hostname)?,
        None => return Err("--id or --id-from-hostname is needed".to_owned()),
    };
    let peers = parse_peers(values.get("--peers").ok_or("--peers is needed")?)?;
    if !peers.contains_key(&NodeId(id)) {
        return Err(format!("--peers does not list this replica ({id})"));
    }
    let (listen_peer, listen_client) = (
        text("--listen-peer", "0.0.0.0:7000"),
        text("--listen-client", "0.0.0.0:8080"),
    );
    for (name, address) in [
        ("--listen-peer", &listen_peer),
        ("--listen-client", &listen_client),
    ] {
        if split_address(address).is_none() {
            return Err(format!("{name}: `{address}` is not HOST:PORT"));
        }
    }
    let (heartbeat_ms, election_ms) = (
        number("--heartbeat-ms", 100)?,
        number("--election-ms", 1_000)?,
    );
    if heartbeat_ms == 0 || heartbeat_ms >= election_ms {
        return Err("--heartbeat-ms must be above 0 and below --election-ms".to_owned());
    }
    if election_ms > MAX_ELECTION_MS {
        return Err(format!("--election-ms may be at most {MAX_ELECTION_MS}"));
    }
    Ok(Options {
        id: NodeId(id),
        peers,
        listen_peer,
        listen_client,
        data_dir: text("--data-dir", "./data").into(),
        airport: match text("--airport", "demo").as_str() {
            "demo" => AirportChoice::Demo,
            "single" => AirportChoice::Single,
            file => AirportChoice::File(file.into()),
        },
        heartbeat_ms,
        election_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEERS: &str = "1=a:7000,2=b:7000,3=[::1]:7001";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_least_that_must_be_said() {
        let options = parse(&args(&["--id", "2", "--peers", PEERS]), &no_env, None).unwrap();
        assert_eq!(options.id, NodeId(2));
        assert_eq!(options.peers.len(), 3);
        assert_eq!(options.peers[&NodeId(3)], "[::1]:7001");
        assert_eq!(options.listen_peer, "0.0.0.0:7000");
        assert_eq!(options.listen_client, "0.0.0.0:8080");
        assert_eq!(options.data_dir, PathBuf::from("./data"));
        assert_eq!(options.airport, AirportChoice::Demo);
        assert_eq!((options.heartbeat_ms, options.election_ms), (100, 1_000));
    }

    #[test]
    fn the_environment_fills_in_and_arguments_win() {
        let env = |name: &str| match name {
            "AIRPORT_PEERS" => Some(PEERS.to_owned()),
            "AIRPORT_ID" => Some("1".to_owned()),
            "AIRPORT_CONFIG" => Some("single".to_owned()),
            "AIRPORT_DATA_DIR" => Some(String::new()),
            _ => None,
        };
        let options = parse(
            &args(&["--id", "3", "--airport", "/etc/a.json"]),
            &env,
            None,
        )
        .unwrap();
        assert_eq!(options.id, NodeId(3));
        assert_eq!(options.airport, AirportChoice::File("/etc/a.json".into()));
        assert_eq!(
            options.data_dir,
            PathBuf::from("./data"),
            "an empty variable is unset"
        );
        assert_eq!(
            parse(&[], &env, None).unwrap().airport,
            AirportChoice::Single
        );
    }

    #[test]
    fn the_hostname_can_name_the_replica() {
        let with = |hostname| {
            parse(
                &args(&["--id-from-hostname", "--peers", PEERS]),
                &no_env,
                hostname,
            )
        };
        assert_eq!(with(Some("airport-0")).unwrap().id, NodeId(1));
        assert_eq!(with(Some("my-release-airport-2")).unwrap().id, NodeId(3));
        assert!(
            with(Some("airport"))
                .unwrap_err()
                .contains("does not end in")
        );
        assert!(with(None).unwrap_err().contains("not known"));
        let env = |name: &str| (name == "AIRPORT_ID_FROM_HOSTNAME").then(|| "1".to_owned());
        assert_eq!(
            parse(&args(&["--peers", PEERS]), &env, Some("a-1"))
                .unwrap()
                .id,
            NodeId(2)
        );
    }

    #[test]
    fn mistakes_are_named() {
        for (list, expected) in [
            (&["--peers", PEERS][..], "--id or --id-from-hostname"),
            (&["--id", "1"], "--peers is needed"),
            (
                &["--id", "x", "--peers", PEERS],
                "--id needs a whole number",
            ),
            (
                &["--id", "4", "--peers", PEERS],
                "does not list this replica",
            ),
            (&["--id", "1", "--peers", "1=a:1,1=b:2"], "listed twice"),
            (&["--id", "1", "--peers", "1=a"], "not HOST:PORT"),
            (&["--id", "1", "--peers", "a:1"], "not ID=HOST:PORT"),
            (
                &["--id", "1", "--peers", PEERS, "--listen-client", "8080"],
                "--listen-client",
            ),
            (
                &["--id", "1", "--peers", PEERS, "--heartbeat-ms", "0"],
                "--heartbeat-ms",
            ),
            (
                &["--id", "1", "--peers", PEERS, "--election-ms", "100"],
                "--heartbeat-ms",
            ),
            (
                &["--id", "1", "--peers", PEERS, "--election-ms", "9999999"],
                "at most",
            ),
            (
                &["--id", "1", "--peers", PEERS, "--bogus"],
                "unexpected argument --bogus",
            ),
            (&["--id"], "--id needs a value"),
        ] {
            let error = parse(&args(list), &no_env, None).unwrap_err();
            assert!(error.contains(expected), "{list:?}: {error}");
        }
    }
}
