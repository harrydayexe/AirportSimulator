//! The traffic generator: a stream of flight requests from a random seed.

use crate::command::Command;
use crate::rng::Rng;
use crate::scenario::Timed;
use crate::{MS_PER_HOUR, Ms};

/// What traffic to generate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrafficParams {
    /// Seed for the generator; the same parameters give the same traffic.
    pub seed: u64,
    /// Flights ask for times in `start_ms..start_ms + duration_ms`.
    pub start_ms: Ms,
    /// Length of the window flights ask for times in.
    pub duration_ms: Ms,
    /// Arrivals requested per hour of `duration_ms`.
    pub arrivals_per_hour: u32,
    /// Departures requested per hour of `duration_ms`.
    pub departures_per_hour: u32,
    /// How long before its requested time a flight files its request.
    pub notice_ms: Ms,
    /// Aircraft types and their relative share of the traffic.
    pub mix: Vec<(String, u32)>,
}

/// A typical mix for a large airport: mostly medium jets, some heavies.
#[must_use]
pub fn default_mix() -> Vec<(String, u32)> {
    [
        ("A320", 28),
        ("B738", 24),
        ("A321", 10),
        ("E190", 8),
        ("AT76", 5),
        ("DH8D", 4),
        ("B788", 5),
        ("A359", 5),
        ("B77W", 4),
        ("A333", 3),
        ("B763", 1),
        ("A388", 2),
        ("PC12", 1),
    ]
    .into_iter()
    .map(|(icao, weight)| (icao.to_owned(), weight))
    .collect()
}

impl TrafficParams {
    /// Traffic starting at time 0, with 20 minutes' notice and
    /// [`default_mix`].
    #[must_use]
    pub fn new(
        seed: u64,
        duration_ms: Ms,
        arrivals_per_hour: u32,
        departures_per_hour: u32,
    ) -> Self {
        Self {
            seed,
            start_ms: 0,
            duration_ms,
            arrivals_per_hour,
            departures_per_hour,
            notice_ms: 20 * 60_000,
            mix: default_mix(),
        }
    }
}

const AIRLINES: [&str; 10] = [
    "BAW", "DLH", "AFR", "KLM", "EZY", "RYR", "UAE", "SAS", "IBE", "TAP",
];

/// Generates flight requests. Each flight asks for a uniformly random time
/// in the window, which makes the arrivals of each kind a Poisson stream.
/// The same parameters always give the same list, ordered by filing time.
#[must_use]
pub fn generate(params: &TrafficParams) -> Vec<Timed> {
    let mut rng = Rng::new(params.seed);
    let total_weight: u64 = params.mix.iter().map(|(_, w)| u64::from(*w)).sum();
    let mut flights = Vec::new();
    let mut number = 100u32;
    for (landing, per_hour) in [
        (true, params.arrivals_per_hour),
        (false, params.departures_per_hour),
    ] {
        let count = (u64::from(per_hour) * params.duration_ms + MS_PER_HOUR / 2) / MS_PER_HOUR;
        for _ in 0..count {
            let earliest_ms = params.start_ms + rng.below(params.duration_ms);
            let aircraft = pick(&params.mix, rng.below(total_weight));
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the index is below AIRLINES.len(), so it fits in a usize"
            )]
            let airline = AIRLINES[rng.below(AIRLINES.len() as u64) as usize];
            let flight = format!("{airline}{number}");
            number += 1;
            let command = if landing {
                Command::RequestLanding {
                    flight,
                    aircraft,
                    earliest_ms,
                }
            } else {
                Command::RequestDeparture {
                    flight,
                    aircraft,
                    earliest_ms,
                }
            };
            flights.push(Timed {
                at_ms: earliest_ms.saturating_sub(params.notice_ms),
                command,
            });
        }
    }
    flights.sort_by_key(|timed| timed.at_ms);
    flights
}

fn pick(mix: &[(String, u32)], mut roll: u64) -> String {
    for (icao, weight) in mix {
        let weight = u64::from(*weight);
        if roll < weight {
            return icao.clone();
        }
        roll -= weight;
    }
    // Only reached with an empty mix.
    "A320".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::aircraft_index;

    #[test]
    fn same_seed_same_traffic() {
        let params = TrafficParams::new(7, 6 * MS_PER_HOUR, 30, 30);
        assert_eq!(generate(&params), generate(&params));
        let other = TrafficParams::new(8, 6 * MS_PER_HOUR, 30, 30);
        assert_ne!(generate(&params), generate(&other));
    }

    #[test]
    fn produces_the_requested_volume_of_known_types() {
        let params = TrafficParams::new(1, 2 * MS_PER_HOUR, 30, 20);
        let traffic = generate(&params);
        assert_eq!(traffic.len(), 100);
        let mut landings = 0;
        for timed in &traffic {
            let (aircraft, earliest_ms) = match &timed.command {
                Command::RequestLanding {
                    aircraft,
                    earliest_ms,
                    ..
                } => {
                    landings += 1;
                    (aircraft, *earliest_ms)
                }
                Command::RequestDeparture {
                    aircraft,
                    earliest_ms,
                    ..
                } => (aircraft, *earliest_ms),
                other => panic!("unexpected command {other:?}"),
            };
            assert!(aircraft_index(aircraft).is_some());
            assert!(earliest_ms < 2 * MS_PER_HOUR);
            assert!(timed.at_ms <= earliest_ms);
        }
        assert_eq!(landings, 60);
        assert!(traffic.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
    }
}
