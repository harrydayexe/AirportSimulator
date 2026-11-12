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

pub use airport;
pub use raft;

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
#[must_use]
pub fn demo_scenario_json(options: DemoOptions) -> String {
    let scenario = scenario::demo(
        options.seed,
        options.hours,
        options.arrivals_per_hour,
        options.departures_per_hour,
    );
    serde_json::to_string(&scenario).unwrap_or_default()
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
}
