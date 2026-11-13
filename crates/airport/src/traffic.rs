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
///
/// # Panics
/// If flights are to be generated but the mix has no weight (T1), or if the
/// result is out of order or reuses an identifier (T3).
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
        if count > 0 {
            assert!(
                total_weight > 0 && params.duration_ms > 0,
                "traffic: {count} flights to generate from an empty mix or window"
            );
        }
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

    // T3: in filing order, filed no later than the time asked for, and every
    // identifier used once.
    let mut ids = std::collections::BTreeSet::new();
    for timed in &flights {
        let (Command::RequestLanding {
            flight,
            earliest_ms,
            ..
        }
        | Command::RequestDeparture {
            flight,
            earliest_ms,
            ..
        }) = &timed.command
        else {
            unreachable!("traffic only generates requests");
        };
        assert!(timed.at_ms <= *earliest_ms, "traffic: {flight} filed late");
        assert!(ids.insert(flight), "traffic: {flight} generated twice");
    }
    assert!(flights.is_sorted_by_key(|timed| timed.at_ms));
    flights
}

/// The type `roll` lands on, with each type covering `weight` values.
fn pick(mix: &[(String, u32)], mut roll: u64) -> String {
    for (icao, weight) in mix {
        let weight = u64::from(*weight);
        if roll < weight {
            return icao.clone();
        }
        roll -= weight;
    }
    // T2: `generate` rolls below the total weight and checks it is not zero.
    unreachable!("traffic: roll beyond the total weight of the mix")
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

    /// `(flight, earliest_ms, aircraft, landing)` of every request.
    fn requests(traffic: &[Timed]) -> Vec<(String, Ms, String, bool)> {
        traffic
            .iter()
            .map(|t| match &t.command {
                Command::RequestLanding {
                    flight,
                    aircraft,
                    earliest_ms,
                } => (flight.clone(), *earliest_ms, aircraft.clone(), true),
                Command::RequestDeparture {
                    flight,
                    aircraft,
                    earliest_ms,
                } => (flight.clone(), *earliest_ms, aircraft.clone(), false),
                other => panic!("unexpected command {other:?}"),
            })
            .collect()
    }

    #[test]
    fn the_flight_count_rounds_to_nearest() {
        let count = |duration_ms, per_hour| {
            generate(&TrafficParams::new(1, duration_ms, per_hour, 0)).len()
        };
        assert_eq!(count(30 * 60_000, 1), 1);
        assert_eq!(count(29 * 60_000, 1), 0);
        assert_eq!(count(0, 1_000), 0);
    }

    #[test]
    #[should_panic(expected = "empty mix")]
    fn an_empty_mix_is_a_bug() {
        let mut params = TrafficParams::new(1, MS_PER_HOUR, 5, 0);
        params.mix = vec![];
        let _ = generate(&params);
    }

    #[test]
    fn flights_are_numbered_from_100_arrivals_first() {
        let traffic = requests(&generate(&TrafficParams::new(4, MS_PER_HOUR, 3, 2)));
        let mut numbers: Vec<u32> = traffic.iter().map(|r| r.0[3..].parse().unwrap()).collect();
        numbers.sort_unstable();
        assert_eq!(numbers, [100, 101, 102, 103, 104]);
        let mut landing: Vec<u32> = traffic
            .iter()
            .filter(|r| r.3)
            .map(|r| r.0[3..].parse().unwrap())
            .collect();
        landing.sort_unstable();
        assert_eq!(landing, [100, 101, 102]);
    }

    #[test]
    fn notice_longer_than_the_wait_files_at_time_zero() {
        let mut params = TrafficParams::new(2, MS_PER_HOUR, 10, 10);
        params.notice_ms = u64::MAX;
        assert!(generate(&params).iter().all(|t| t.at_ms == 0));
    }

    #[test]
    fn requested_times_fall_in_the_window() {
        let mut params = TrafficParams::new(5, MS_PER_HOUR, 30, 30);
        params.start_ms = 1_000_000;
        let window = 1_000_000..1_000_000 + MS_PER_HOUR;
        assert!(
            requests(&generate(&params))
                .iter()
                .all(|r| window.contains(&r.1))
        );
    }

    #[test]
    fn a_single_type_mix_and_a_zero_weight() {
        let mut params = TrafficParams::new(6, MS_PER_HOUR, 20, 20);
        params.mix = vec![("A320".into(), 0), ("B738".into(), 5)];
        assert!(requests(&generate(&params)).iter().all(|r| r.2 == "B738"));
    }

    #[test]
    fn pick_lands_on_the_right_side_of_each_boundary() {
        let mix = vec![("FIRST".to_owned(), 3), ("SECOND".to_owned(), 2)];
        let picks: Vec<String> = [0, 2, 3, 4].iter().map(|r| pick(&mix, *r)).collect();
        assert_eq!(picks, ["FIRST", "FIRST", "SECOND", "SECOND"]);
    }
}
