//! WebAssembly bindings for the web app. Keep this layer thin: it converts
//! types and forwards calls to the host, and holds no logic of its own.

use host::airport;
use wasm_bindgen::prelude::wasm_bindgen;

/// See [`airport::gap_ms`]. Returns `undefined` for a speed of zero, or if
/// the result does not fit in a `u32`.
#[wasm_bindgen(js_name = gapMs)]
#[must_use]
pub fn gap_ms(distance_nm: u32, speed_kt: u32) -> Option<u32> {
    airport::gap_ms(distance_nm, speed_kt).and_then(|ms| u32::try_from(ms).ok())
}
