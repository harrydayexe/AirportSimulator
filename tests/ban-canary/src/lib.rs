//! Deliberately breaks every determinism rule once. Clippy must refuse it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// One of each banned item.
pub fn everything_banned(x: f64) -> f64 {
    let _now = Instant::now();
    let _map: HashMap<u8, u8> = HashMap::new();
    let _lock = Mutex::new(0u8);
    let _ = std::thread::spawn(|| {});
    x + 1.0
}
