//! WebAssembly bindings for the web app. Keep this layer thin: it converts
//! types and forwards calls, and holds no logic of its own.

use host::airport::scenario::{self, Scenario};
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

/// The demonstration scenario (see `airport::scenario::demo`) as JSON.
/// `seed` is a full `u64` (a `bigint` in JavaScript), so any seed the
/// command line prints can be replayed in the browser.
///
/// # Panics
/// Never: a `Scenario` always serialises.
#[wasm_bindgen(js_name = demoScenario)]
#[must_use]
pub fn demo_scenario(
    seed: u64,
    hours: u32,
    arrivals_per_hour: u32,
    departures_per_hour: u32,
) -> String {
    let scenario = scenario::demo(seed, hours, arrivals_per_hour, departures_per_hour);
    serde_json::to_string(&scenario).expect("a Scenario always serialises")
}

/// Runs a scenario given as JSON through one airport (see
/// `airport::scenario::run`) and returns the result as JSON.
///
/// # Errors
/// Throws if the JSON is not a scenario or its airport is invalid.
#[wasm_bindgen(js_name = runScenario)]
pub fn run_scenario(scenario_json: &str) -> Result<String, JsError> {
    let scenario: Scenario = serde_json::from_str(scenario_json)
        .map_err(|e| JsError::new(&format!("invalid scenario: {e}")))?;
    let result =
        scenario::run(&scenario).map_err(|e| JsError::new(&format!("invalid airport: {e}")))?;
    serde_json::to_string(&result).map_err(|e| JsError::new(&e.to_string()))
}
