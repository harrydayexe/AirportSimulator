//! The host (Phase 2).
//!
//! Runs every replica and controller inside one deterministic event loop,
//! playing the part of the network, disks and clock. Wrapped twice: by the
//! `cli` crate for bulk seeded runs, and by the `wasm` crate for the browser.
//!
//! Until the replicas arrive, the host runs a single airport: it turns a
//! scenario written as JSON into the outputs of that airport, also as JSON.
//! Both wrappers go through the functions here, so the command line and the
//! browser run exactly the same code.

mod hash;
pub use airport;
pub use raft;
pub mod browserhost;
pub mod childhost;
pub mod clock;
pub mod event;
pub mod host;
pub mod queue;
pub mod report;
pub mod seeds;

use airport::scenario::{self, RunResult, Scenario};

/// Options for the demonstration scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoOptions {
    /// Seed for the generator that places the flights.
    pub seed: u64,
    /// How many hours of traffic to generate.
    pub hours: u32,
    /// Arriving flights generated per hour.
    pub arrivals_per_hour: u32,
    /// Departing flights generated per hour.
    pub departures_per_hour: u32,
}

impl Default for DemoOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            hours: 6,
            arrivals_per_hour: 40,
            departures_per_hour: 40,
        }
    }
}

/// The demonstration scenario (see [`scenario::demo`]) as JSON.
///
/// # Panics
/// Never: a [`Scenario`] always serialises.
#[must_use]
pub fn demo_scenario_json(options: DemoOptions) -> String {
    let scenario = scenario::demo(
        options.seed,
        options.hours,
        options.arrivals_per_hour,
        options.departures_per_hour,
    );
    serde_json::to_string(&scenario).expect("a Scenario always serialises")
}

/// Runs a scenario.
///
/// # Errors
/// Returns a message if the JSON is not a scenario or its airport is invalid.
pub fn run_scenario(scenario_json: &str) -> Result<RunResult, String> {
    let scenario: Scenario =
        serde_json::from_str(scenario_json).map_err(|e| format!("invalid scenario: {e}"))?;
    scenario::run(&scenario).map_err(|e| format!("invalid airport: {e}"))
}

/// Runs a scenario given as JSON and returns the [`RunResult`] as JSON.
///
/// # Errors
/// See [`run_scenario`].
pub fn run_scenario_json(scenario_json: &str) -> Result<String, String> {
    let result = run_scenario(scenario_json)?;
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use airport::command::Command;

    use super::*;

    #[test]
    fn runs_the_demo_from_json_the_same_way_twice() {
        let scenario = demo_scenario_json(DemoOptions::default());
        let first = run_scenario_json(&scenario).unwrap();
        assert_eq!(run_scenario_json(&scenario).unwrap(), first);
        assert!(first.contains("\"fingerprint\""));
    }

    #[test]
    fn reports_bad_input() {
        assert!(
            run_scenario_json("{")
                .unwrap_err()
                .starts_with("invalid scenario")
        );
        let no_runways = r#"{"airport":{"name":"X","runways":[]},"commands":[]}"#;
        assert!(
            run_scenario_json(no_runways)
                .unwrap_err()
                .starts_with("invalid airport")
        );
    }

    fn demo_json(options: DemoOptions) -> Scenario {
        serde_json::from_str(&demo_scenario_json(options)).unwrap()
    }

    #[test]
    fn the_demo_json_is_the_demo_scenario() {
        let parsed = demo_json(DemoOptions::default());
        assert_eq!(parsed, scenario::demo(1, 6, 40, 40));
        let count = |f: fn(&Command) -> bool| parsed.commands.iter().filter(|c| f(c)).count();
        assert_eq!(count(|c| matches!(c, Command::SetWind { .. })), 3);
        assert_eq!(count(|c| matches!(c, Command::CloseRunway { .. })), 1);
        assert_eq!(count(|c| matches!(c, Command::OpenRunway { .. })), 1);
        assert_eq!(count(|c| matches!(c, Command::DeclareEmergency { .. })), 1);
        assert_eq!(
            count(|c| matches!(
                c,
                Command::RequestLanding { .. } | Command::RequestDeparture { .. }
            )),
            480
        );
        let last_tick = parsed.commands.iter().rev().find_map(|c| match c {
            Command::AdvanceTime { now_ms } => Some(*now_ms),
            _ => None,
        });
        assert_eq!(last_tick, Some(28_800_000));
    }

    #[test]
    fn zero_hours_means_one_hour() {
        let with = |hours| {
            demo_scenario_json(DemoOptions {
                hours,
                ..DemoOptions::default()
            })
        };
        assert_eq!(with(0), with(1));
    }

    #[test]
    fn a_demo_with_no_traffic_still_runs() {
        let options = DemoOptions {
            arrivals_per_hour: 0,
            departures_per_hour: 0,
            ..DemoOptions::default()
        };
        let parsed = demo_json(options);
        assert!(
            !parsed
                .commands
                .iter()
                .any(|c| matches!(c, Command::DeclareEmergency { .. }))
        );
        let result = run_scenario(&demo_scenario_json(options)).unwrap();
        assert_eq!(result.end.stats, airport::state::Stats::default());
    }

    #[test]
    fn json_of_the_wrong_shape_is_an_invalid_scenario() {
        for json in ["{}", "[]", ""] {
            let error = run_scenario_json(json).unwrap_err();
            assert!(error.starts_with("invalid scenario"), "{json:?}: {error}");
        }
    }

    #[test]
    fn each_bad_airport_names_its_problem() {
        use airport::{AirportConfig, ConfigError, RunwayConfig};
        let runway = |low: &str, high: &str, heading| RunwayConfig::new(low, high, heading, 3_000);
        let mut duplicate = AirportConfig::demo();
        duplicate.runways[2].low = "09L".into();
        let mut heading = AirportConfig::single_runway();
        heading.runways[0].heading_deg = 360;
        let mut crossing = AirportConfig::demo();
        crossing.crossings.push(("18".into(), "04".into()));
        let many = AirportConfig::new(
            "Many",
            (0..13)
                .map(|i| runway(&format!("L{i}"), &format!("H{i}"), 90))
                .collect(),
        );
        for (config, error) in [
            (duplicate, ConfigError::DuplicateRunwayName("09L".into())),
            (heading, ConfigError::BadHeading("09/27".into())),
            (crossing, ConfigError::UnknownCrossing("04".into())),
            (many, ConfigError::TooManyRunways),
        ] {
            let scenario = Scenario {
                airport: config,
                commands: Vec::new(),
            };
            let message =
                run_scenario_json(&serde_json::to_string(&scenario).unwrap()).unwrap_err();
            assert!(message.starts_with("invalid airport: "), "{message}");
            assert!(message.contains(&error.to_string()), "{message}");
        }
    }

    #[test]
    fn the_two_entry_points_agree() {
        let json = demo_scenario_json(DemoOptions {
            hours: 1,
            ..DemoOptions::default()
        });
        let result = run_scenario(&json).unwrap();
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            run_scenario_json(&json).unwrap()
        );
    }
}
