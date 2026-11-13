//! The data tables behind the four modelled aspects of real operations
//! (design/design-proposal.md, Phase 1). Every number the rules use lives
//! here, not in the logic.
//!
//! Sources: ICAO Doc 4444 (PANS-ATM) chapter 4 for the wake turbulence
//! categories, chapters 5, 6 and 8 for the separation minima, and the ICAO
//! A380 wake guidance for the "super" rows. Approach speeds and runway
//! lengths are representative published figures per type. Check a value
//! against the rule book before changing it.

use serde::{Deserialize, Serialize};

use crate::Ms;

/// ICAO wake turbulence category, lightest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeClass {
    /// 7 000 kg or less.
    Light,
    /// Between 7 000 kg and 136 000 kg.
    Medium,
    /// 136 000 kg or more.
    Heavy,
    /// The Airbus A380.
    Super,
}

impl WakeClass {
    const fn index(self) -> usize {
        self as usize
    }
}

/// Minimum radar separation on approach where wake is not the limit, in
/// nautical miles (Doc 4444, 8.7.3).
pub const MIN_RADAR_SEPARATION_NM: u32 = 3;

/// Wake separation on approach in nautical miles, `[leader][follower]`.
/// Zero means wake sets no minimum for that pair (Doc 4444, 8.7.3.4, plus
/// the A380 guidance).
const WAKE_APPROACH_NM: [[u32; 4]; 4] = [
    // follower: Light, Medium, Heavy, Super
    [0, 0, 0, 0], // behind a Light
    [5, 0, 0, 0], // behind a Medium
    [6, 5, 4, 0], // behind a Heavy
    [8, 7, 6, 0], // behind a Super
];

/// Required distance between two arrivals on the same runway, in nautical
/// miles: the wake minimum or the radar minimum, whichever is greater.
#[must_use]
pub fn approach_separation_nm(leader: WakeClass, follower: WakeClass) -> u32 {
    WAKE_APPROACH_NM[leader.index()][follower.index()].max(MIN_RADAR_SEPARATION_NM)
}

/// Minimum time between two departures from the same runway where wake is
/// not the limit (Doc 4444, 5.6: one-minute separation).
pub const MIN_DEPARTURE_INTERVAL_MS: Ms = 60_000;

/// Wake separation between departures in milliseconds, `[leader][follower]`
/// (Doc 4444, 5.8.3, plus the A380 guidance). Zero means no wake minimum.
const WAKE_DEPARTURE_MS: [[Ms; 4]; 4] = [
    // follower: Light, Medium, Heavy, Super
    [0, 0, 0, 0],                   // behind a Light
    [120_000, 0, 0, 0],             // behind a Medium
    [120_000, 120_000, 0, 0],       // behind a Heavy
    [180_000, 180_000, 120_000, 0], // behind a Super
];

/// Required time between two departures from the same runway: the wake
/// minimum or the general minimum, whichever is greater.
#[must_use]
pub fn departure_separation_ms(leader: WakeClass, follower: WakeClass) -> Ms {
    WAKE_DEPARTURE_MS[leader.index()][follower.index()].max(MIN_DEPARTURE_INTERVAL_MS)
}

/// Time an arrival occupies the runway from crossing the threshold to
/// turning off, by wake class, for a runway with well-placed exits. A
/// runway's `exit_extra_ms` is added on top.
const ARRIVAL_OCCUPANCY_MS: [Ms; 4] = [40_000, 50_000, 55_000, 60_000];

/// Time a departure occupies the runway from starting its roll to lifting
/// off, by wake class.
const DEPARTURE_OCCUPANCY_MS: [Ms; 4] = [30_000, 40_000, 50_000, 55_000];

/// The shortest time any movement occupies a runway.
pub const MIN_OCCUPANCY_MS: Ms = 30_000;

/// See [`ARRIVAL_OCCUPANCY_MS`].
#[must_use]
pub fn arrival_occupancy_ms(class: WakeClass) -> Ms {
    ARRIVAL_OCCUPANCY_MS[class.index()]
}

/// See [`DEPARTURE_OCCUPANCY_MS`].
#[must_use]
pub fn departure_occupancy_ms(class: WakeClass) -> Ms {
    DEPARTURE_OCCUPANCY_MS[class.index()]
}

/// How long a runway stays closed for inspection after a mayday landing.
pub const DEFAULT_MAYDAY_INSPECTION_MS: Ms = 15 * 60_000;

/// How long an arrival can hold before it gives up and diverts.
pub const DEFAULT_MAX_ARRIVAL_HOLD_MS: Ms = 45 * 60_000;

/// Length of the common final approach path, in nautical miles: where a 3
/// degree glide path meets 2 000 ft.
pub const DEFAULT_FINAL_APPROACH_NM: u32 = 6;

/// Crosswind limit used for a runway unless it sets its own, in knots.
pub const DEFAULT_CROSSWIND_LIMIT_KT: u16 = 25;

/// Latest simulated time an airport accepts: about 100 years. Every time the
/// rules add to stays far enough below `u64::MAX` that no sum can overflow.
pub const MAX_TIME_MS: Ms = 100 * 365 * 24 * 3_600_000;

/// How far ahead a flight may ask for a slot: 120 hours, the earliest a
/// flight plan can be filed before departure (ICAO Doc 4444, EUROCONTROL
/// IFPS).
pub const MAX_FILING_AHEAD_MS: Ms = 120 * 3_600_000;

/// Upper bounds on the configurable times and distances, so a configuration
/// file cannot push the arithmetic past `u64` (see [`MAX_TIME_MS`]).
pub const MAX_EXIT_EXTRA_MS: Ms = 10 * 60_000;
/// See [`MAX_EXIT_EXTRA_MS`].
pub const MAX_MAYDAY_INSPECTION_MS: Ms = 24 * 3_600_000;
/// See [`MAX_EXIT_EXTRA_MS`].
pub const MAX_ARRIVAL_HOLD_MS: Ms = 24 * 3_600_000;
/// See [`MAX_EXIT_EXTRA_MS`].
pub const MAX_FINAL_APPROACH_NM: u32 = 50;

/// The short performance table kept for each aircraft type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AircraftType {
    /// ICAO type designator, e.g. `A320`.
    pub icao: &'static str,
    /// Wake turbulence class, which sets the separation behind it.
    pub wake: WakeClass,
    /// Final approach speed in knots.
    pub approach_speed_kt: u32,
    /// Runway length the type needs, in metres.
    pub runway_m: u32,
}

const fn ac(icao: &'static str, wake: WakeClass, kt: u32, runway_m: u32) -> AircraftType {
    AircraftType {
        icao,
        wake,
        approach_speed_kt: kt,
        runway_m,
    }
}

/// Every aircraft type the simulator knows. Saved state refers to a type by
/// its position here, so append new types at the end.
pub const AIRCRAFT_TYPES: [AircraftType; 14] = [
    ac("C172", WakeClass::Light, 65, 500),
    ac("PC12", WakeClass::Light, 85, 800),
    ac("AT76", WakeClass::Medium, 113, 1_350),
    ac("DH8D", WakeClass::Medium, 122, 1_400),
    ac("E190", WakeClass::Medium, 124, 1_700),
    ac("A320", WakeClass::Medium, 137, 2_000),
    ac("B738", WakeClass::Medium, 144, 2_300),
    ac("A321", WakeClass::Medium, 141, 2_300),
    ac("B763", WakeClass::Heavy, 140, 2_500),
    ac("A333", WakeClass::Heavy, 137, 2_500),
    ac("B788", WakeClass::Heavy, 140, 2_600),
    ac("A359", WakeClass::Heavy, 140, 2_600),
    ac("B77W", WakeClass::Heavy, 149, 3_000),
    ac("A388", WakeClass::Super, 138, 3_000),
];

/// Position of a type in [`AIRCRAFT_TYPES`], by ICAO designator.
#[must_use]
pub fn aircraft_index(icao: &str) -> Option<u16> {
    AIRCRAFT_TYPES
        .iter()
        .position(|t| t.icao == icao)
        .and_then(|i| u16::try_from(i).ok())
}

/// The type at a position previously returned by [`aircraft_index`].
///
/// # Panics
/// If `index` is not a position in [`AIRCRAFT_TYPES`] (D7).
#[must_use]
pub fn aircraft(index: u16) -> &'static AircraftType {
    AIRCRAFT_TYPES.get(usize::from(index)).unwrap_or_else(|| {
        panic!(
            "aircraft index {index} is outside the table of {}",
            AIRCRAFT_TYPES.len()
        )
    })
}

/// `sin(d degrees) * 10 000`, rounded, for d in 0..=90. A fixed table because
/// the platform's sine may differ between machines (design section 4).
pub const SIN_E4: [u32; 91] = [
    0, 175, 349, 523, 698, 872, 1045, 1219, 1392, 1564, //
    1736, 1908, 2079, 2250, 2419, 2588, 2756, 2924, 3090, 3256, //
    3420, 3584, 3746, 3907, 4067, 4226, 4384, 4540, 4695, 4848, //
    5000, 5150, 5299, 5446, 5592, 5736, 5878, 6018, 6157, 6293, //
    6428, 6561, 6691, 6820, 6947, 7071, 7193, 7314, 7431, 7547, //
    7660, 7771, 7880, 7986, 8090, 8192, 8290, 8387, 8480, 8572, //
    8660, 8746, 8829, 8910, 8988, 9063, 9135, 9205, 9272, 9336, //
    9397, 9455, 9511, 9563, 9613, 9659, 9703, 9744, 9781, 9816, //
    9848, 9877, 9903, 9925, 9945, 9962, 9976, 9986, 9994, 9998, //
    10000,
];

// Facts about the tables above that the rules rely on. Checked when the
// crate is built, so a bad edit to a table fails the build, not a run.
const _: () = {
    // D1, D2: `approach_gap_ms` divides by the approach speed; every type
    // needs some runway; and a position fits the `u16` saved state uses.
    assert!(AIRCRAFT_TYPES.len() <= u16::MAX as usize);
    let mut i = 0;
    while i < AIRCRAFT_TYPES.len() {
        assert!(AIRCRAFT_TYPES[i].approach_speed_kt > 0);
        assert!(AIRCRAFT_TYPES[i].runway_m > 0);
        i += 1;
    }

    // D3: `sealed` is only sound if no movement is shorter than this.
    let mut smallest = Ms::MAX;
    let mut found = false;
    let mut c = 0;
    while c < 4 {
        let (a, d) = (ARRIVAL_OCCUPANCY_MS[c], DEPARTURE_OCCUPANCY_MS[c]);
        smallest = if a < smallest { a } else { smallest };
        smallest = if d < smallest { d } else { smallest };
        found = found || a == MIN_OCCUPANCY_MS || d == MIN_OCCUPANCY_MS;
        c += 1;
    }
    assert!(found && smallest == MIN_OCCUPANCY_MS);

    // D4: the sine table runs from 0 to 1 and never falls.
    assert!(SIN_E4[0] == 0 && SIN_E4[90] == 10_000);
    let mut d = 1;
    while d < SIN_E4.len() {
        assert!(SIN_E4[d - 1] <= SIN_E4[d]);
        d += 1;
    }

    // D5: a heavier follower never needs more room than a lighter one
    // behind the same leader.
    let mut leader = 0;
    while leader < 4 {
        let mut follower = 1;
        while follower < 4 {
            assert!(WAKE_APPROACH_NM[leader][follower] <= WAKE_APPROACH_NM[leader][follower - 1]);
            assert!(WAKE_DEPARTURE_MS[leader][follower] <= WAKE_DEPARTURE_MS[leader][follower - 1]);
            follower += 1;
        }
        leader += 1;
    }

    // The time cap leaves room for every sum the rules make on top of it.
    assert!(
        MAX_TIME_MS
            < Ms::MAX / 4
                - MAX_FILING_AHEAD_MS
                - MAX_ARRIVAL_HOLD_MS
                - MAX_MAYDAY_INSPECTION_MS
                - MAX_EXIT_EXTRA_MS
    );
};

#[cfg(test)]
mod tests {
    use super::WakeClass::{Heavy, Light, Medium, Super};
    use super::*;

    #[test]
    fn approach_separation_matches_doc_4444() {
        assert_eq!(approach_separation_nm(Heavy, Light), 6);
        assert_eq!(approach_separation_nm(Heavy, Medium), 5);
        assert_eq!(approach_separation_nm(Heavy, Heavy), 4);
        assert_eq!(approach_separation_nm(Medium, Light), 5);
        assert_eq!(approach_separation_nm(Super, Heavy), 6);
        assert_eq!(approach_separation_nm(Super, Medium), 7);
        assert_eq!(approach_separation_nm(Super, Light), 8);
    }

    #[test]
    fn radar_minimum_applies_where_wake_does_not() {
        for (leader, follower) in [
            (Light, Light),
            (Light, Heavy),
            (Medium, Medium),
            (Medium, Heavy),
            (Heavy, Super),
            (Super, Super),
        ] {
            assert_eq!(approach_separation_nm(leader, follower), 3);
        }
    }

    #[test]
    fn departure_separation_matches_doc_4444() {
        assert_eq!(departure_separation_ms(Heavy, Medium), 120_000);
        assert_eq!(departure_separation_ms(Heavy, Light), 120_000);
        assert_eq!(departure_separation_ms(Medium, Light), 120_000);
        assert_eq!(departure_separation_ms(Super, Medium), 180_000);
        assert_eq!(departure_separation_ms(Super, Heavy), 120_000);
        assert_eq!(departure_separation_ms(Medium, Medium), 60_000);
        assert_eq!(departure_separation_ms(Heavy, Heavy), 60_000);
    }

    #[test]
    fn aircraft_table_is_consistent() {
        for (i, t) in AIRCRAFT_TYPES.iter().enumerate() {
            assert_eq!(aircraft_index(t.icao), u16::try_from(i).ok());
            assert!(t.approach_speed_kt > 0);
        }
        assert_eq!(aircraft_index("ZZZZ"), None);
        assert_eq!(aircraft(aircraft_index("A388").unwrap()).wake, Super);
    }
}
