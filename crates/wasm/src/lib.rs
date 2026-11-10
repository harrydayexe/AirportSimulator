//! WebAssembly bindings for the web app. Keep this layer thin: it converts
//! types and forwards calls to the host, and holds no logic of its own.

use host::DemoOptions;
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

/// The demonstration scenario as JSON. See `host::demo_scenario_json`.
#[wasm_bindgen(js_name = demoScenario)]
#[must_use]
pub fn demo_scenario(
    seed: u32,
    hours: u32,
    arrivals_per_hour: u32,
    departures_per_hour: u32,
) -> String {
    host::demo_scenario_json(DemoOptions {
        seed: u64::from(seed),
        hours,
        arrivals_per_hour,
        departures_per_hour,
    })
}

/// Runs a scenario given as JSON and returns the result as JSON. See
/// `host::run_scenario_json`.
///
/// # Errors
/// Throws if the JSON is not a scenario or its airport is invalid.
#[wasm_bindgen(js_name = runScenario)]
pub fn run_scenario(scenario_json: &str) -> Result<String, JsError> {
    host::run_scenario_json(scenario_json).map_err(|message| JsError::new(&message))
}
