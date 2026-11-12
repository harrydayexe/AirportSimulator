//! Random-input testing of the airport state machine (design section 8,
//! layer 2). Each seed invents an airport and a jumble of commands, then a
//! checker replays the outputs and looks for a broken rule:
//!
//! - **Separation.** Two aircraft never use a runway closer together than
//!   the tables require for that pair.
//! - **One at a time.** A runway never has two aircraft on it at once.
//! - **Closures respected.** Nothing moves on a closed runway, during an
//!   inspection or during a ground stop.
//! - **Layout respected.** Nothing moves on a runway that is not in use, or
//!   while an aircraft is still on a runway that crosses it.
//! - **No lost flights, no duplicated flights.** Every accepted flight ends
//!   exactly once: landed, departed or diverted.
//!
//! The checker shares the data tables with the airport but none of its
//! logic. A failure prints its seed; rerun with that seed to replay it.

use std::collections::BTreeMap;

use airport::command::DivertReason;
use airport::data::{
    AIRCRAFT_TYPES, AircraftType, aircraft, aircraft_index, approach_separation_nm,
    arrival_occupancy_ms, departure_occupancy_ms, departure_separation_ms,
};
use airport::rng::Rng;
use airport::scenario::{Scenario, Timed, run, script};
use airport::{
    Airport, AirportConfig, Command, Emergency, MS_PER_HOUR, MS_PER_MINUTE, Ms, Output, Reason,
    RunwayConfig,
};

const REASONS: [Reason; 5] = [
    Reason::Maintenance,
    Reason::Storm,
    Reason::Curfew,
    Reason::Incident,
    Reason::Other,
];

/// A random value in `0..n`, in whatever integer type the caller needs.
fn below<T: TryFrom<u64>>(rng: &mut Rng, n: u64) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(rng.below(n)).expect("a value below n fits the target type")
}

/// A random position in a list of `len` items.
fn index(rng: &mut Rng, len: usize) -> usize {
    below(rng, len as u64)
}

fn random_airport(rng: &mut Rng) -> AirportConfig {
    let mut config = match rng.below(4) {
        0 => AirportConfig::single_runway(),
        1 => AirportConfig::demo(),
        _ => {
            let count = 1 + rng.below(4);
            let mut runways = Vec::new();
            for i in 0..count {
                let heading = below(rng, 180);
                let mut runway = RunwayConfig::new(
                    &format!("L{i}"),
                    &format!("H{i}"),
                    heading,
                    1_500 + 500 * below::<u32>(rng, 6),
                );
                runway.exit_extra_ms = 5_000 * rng.below(4);
                runway.crosswind_limit_kt = 15 + below::<u16>(rng, 20);
                runways.push(runway);
            }
            let mut config = AirportConfig::new("Random", runways);
            for a in 0..count {
                for b in a + 1..count {
                    if rng.below(3) == 0 {
                        config.crossings.push((format!("L{a}"), format!("H{b}")));
                    }
                }
            }
            config
        }
    };
    config.final_approach_nm = 3 + below::<u32>(rng, 10);
    config.mayday_inspection_ms = rng.below(20) * MS_PER_MINUTE;
    config.max_arrival_hold_ms = match rng.below(3) {
        0 => None,
        _ => Some((10 + rng.below(60)) * MS_PER_MINUTE),
    };
    config
}

fn random_scenario(seed: u64) -> Scenario {
    let mut rng = Rng::new(seed);
    let airport = random_airport(&mut rng);
    let duration = (1 + rng.below(6)) * MS_PER_HOUR;
    let mut timed = Vec::new();
    let mut flights = Vec::new();

    for i in 0..rng.below(400) {
        let flight = format!("F{i}");
        let aircraft = AIRCRAFT_TYPES[index(&mut rng, AIRCRAFT_TYPES.len())]
            .icao
            .to_owned();
        let earliest_ms = rng.below(duration);
        let at_ms = earliest_ms.saturating_sub(rng.below(30 * MS_PER_MINUTE));
        let command = if rng.below(2) == 0 {
            Command::RequestLanding {
                flight: flight.clone(),
                aircraft,
                earliest_ms,
            }
        } else {
            Command::RequestDeparture {
                flight: flight.clone(),
                aircraft,
                earliest_ms,
            }
        };
        // Now and then the same request is sent twice.
        if rng.below(20) == 0 {
            timed.push(Timed {
                at_ms: at_ms + rng.below(MS_PER_MINUTE),
                command: command.clone(),
            });
        }
        timed.push(Timed { at_ms, command });
        flights.push((flight, at_ms));
    }

    for _ in 0..rng.below(40) {
        let at_ms = rng.below(duration);
        let runway = &airport.runways[index(&mut rng, airport.runways.len())];
        let reason = REASONS[index(&mut rng, REASONS.len())];
        let command = match rng.below(9) {
            0 | 1 => Command::CloseRunway {
                runway: runway.low.clone(),
                reason,
            },
            2 | 3 => Command::OpenRunway {
                runway: runway.name(),
                reason,
            },
            4 | 5 => Command::SetWind {
                direction_deg: below(&mut rng, 361),
                speed_kt: below(&mut rng, 45),
            },
            6 => Command::GroundStop { reason },
            7 => Command::LiftGroundStop { reason },
            _ if !flights.is_empty() => {
                let (flight, filed_ms) = &flights[index(&mut rng, flights.len())];
                timed.push(Timed {
                    at_ms: filed_ms + rng.below(40 * MS_PER_MINUTE),
                    command: Command::DeclareEmergency {
                        flight: flight.clone(),
                        kind: [Emergency::PanPan, Emergency::Mayday][index(&mut rng, 2)],
                    },
                });
                continue;
            }
            _ => continue,
        };
        timed.push(Timed { at_ms, command });
    }

    // Once the faults have cleared, every flight must finish: lift every
    // restriction, calm the wind and let the queue drain.
    let clear_ms = duration + 40 * MS_PER_MINUTE;
    for reason in REASONS {
        timed.push(Timed {
            at_ms: clear_ms,
            command: Command::LiftGroundStop { reason },
        });
        for runway in &airport.runways {
            timed.push(Timed {
                at_ms: clear_ms,
                command: Command::OpenRunway {
                    runway: runway.name(),
                    reason,
                },
            });
        }
    }
    timed.push(Timed {
        at_ms: clear_ms,
        command: Command::SetWind {
            direction_deg: 0,
            speed_kt: 0,
        },
    });

    let tick_ms = [1_000, 5_000, 30_000, 4 * MS_PER_MINUTE][index(&mut rng, 4)];
    Scenario {
        airport,
        commands: script(timed, tick_ms, clear_ms + 30 * MS_PER_HOUR),
    }
}

#[derive(Clone, Copy)]
struct Movement {
    at_ms: Ms,
    vacated_ms: Ms,
    ty: &'static AircraftType,
}

#[derive(Default)]
struct RunwayRecord {
    last_arrival: Option<Movement>,
    last_departure: Option<Movement>,
    /// When the runway last became usable, or `None` while it is closed.
    open_since: Option<Ms>,
    inspection_until: Ms,
}

/// Required time between two arrivals, worked out from the tables without
/// using the airport's own code.
fn arrival_gap(leader: &AircraftType, follower: &AircraftType, final_nm: u64) -> Ms {
    let sep = u64::from(approach_separation_nm(leader.wake, follower.wake));
    let vl = u64::from(leader.approach_speed_kt);
    let vf = u64::from(follower.approach_speed_kt);
    // Milliseconds for `nm` at `kt`, as an exact fraction over `kt`.
    let ms = |nm: u64| nm * MS_PER_HOUR;
    if vf >= vl {
        ms(sep).div_ceil(vf)
    } else {
        // Must hold when the leader is at the start of the final approach:
        // follower's time for (final + sep) minus leader's time for final.
        let follower_scaled = ms(final_nm + sep) * vl;
        let leader_scaled = ms(final_nm) * vf;
        (follower_scaled - leader_scaled).div_ceil(vl * vf)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one pass over the outputs checks every rule; splitting it scatters the shared state"
)]
fn check(seed: u64, scenario: &Scenario, outputs: &[Output], end: &Airport) {
    let config = &scenario.airport;
    let crossing = config.validate().unwrap();
    let final_nm = u64::from(config.final_approach_nm);
    let fail = |what: &str, detail: &dyn std::fmt::Debug| -> ! {
        panic!("seed {seed}: {what}: {detail:?}");
    };

    let mut runways: Vec<RunwayRecord> = config
        .runways
        .iter()
        .map(|_| RunwayRecord {
            open_since: Some(0),
            ..RunwayRecord::default()
        })
        .collect();
    let mut active: Vec<String> = Airport::new(config.clone()).unwrap().active_runways();
    let mut ground_stopped = false;
    let mut ground_clear_since: Ms = 0;
    let mut accepted: BTreeMap<&str, u32> = BTreeMap::new();
    let mut maydays: Vec<&str> = Vec::new();

    for output in outputs {
        match output {
            Output::Accepted { flight, .. } => {
                if accepted.insert(flight, 0).is_some() {
                    fail("flight accepted twice", output);
                }
            }
            Output::EmergencyDeclared { flight, kind, .. } => {
                if *kind == Emergency::Mayday {
                    maydays.push(flight);
                }
            }
            Output::Diverted { flight, reason, .. } => {
                let DivertReason::HoldLimit = reason;
                if config.max_arrival_hold_ms.is_none() {
                    fail("diverted with no hold limit", output);
                }
                match accepted.get_mut(flight.as_str()) {
                    Some(ended) => *ended += 1,
                    None => fail("unknown flight diverted", output),
                }
            }
            Output::RunwayClosed { runway, .. } => {
                let r = config.runway_index(runway).unwrap();
                runways[r].open_since = None;
            }
            Output::RunwayOpened {
                runway,
                at_ms,
                open,
                ..
            } => {
                let r = config.runway_index(runway).unwrap();
                if *open {
                    runways[r].open_since = Some(*at_ms);
                }
            }
            Output::RunwayInspection {
                runway, until_ms, ..
            } => {
                let r = config.runway_index(runway).unwrap();
                runways[r].inspection_until = *until_ms;
            }
            Output::ActiveRunways { runways, .. } => active.clone_from(runways),
            Output::GroundStopStarted { .. } => ground_stopped = true,
            Output::GroundStopLifted { at_ms, clear, .. } => {
                if *clear {
                    ground_stopped = false;
                    ground_clear_since = *at_ms;
                }
            }
            Output::Landed {
                flight,
                aircraft: code,
                runway,
                at_ms,
                vacated_ms,
                ..
            }
            | Output::Departed {
                flight,
                aircraft: code,
                runway,
                at_ms,
                vacated_ms,
                ..
            } => {
                let landing = matches!(output, Output::Landed { .. });
                let ty = aircraft(aircraft_index(code).unwrap());
                let r = config.runway_index(runway).unwrap();
                let movement = Movement {
                    at_ms: *at_ms,
                    vacated_ms: *vacated_ms,
                    ty,
                };

                match accepted.get_mut(flight.as_str()) {
                    Some(ended) => *ended += 1,
                    None => fail("unknown flight moved", output),
                }
                if config.runways[r].length_m < ty.runway_m {
                    fail("runway too short", output);
                }
                // Closures respected.
                if ground_stopped || *at_ms < ground_clear_since {
                    fail("moved during a ground stop", output);
                }
                match runways[r].open_since {
                    Some(since) if *at_ms >= since => {}
                    _ => fail("moved on a closed runway", output),
                }
                if *at_ms < runways[r].inspection_until {
                    fail("moved during an inspection", output);
                }
                // Layout respected.
                if !active.contains(runway) {
                    fail("moved on a runway not in use", output);
                }
                for (c, other) in runways.iter().enumerate() {
                    if crossing[r] & (1 << c) == 0 {
                        continue;
                    }
                    for last in [other.last_arrival, other.last_departure] {
                        if last.is_some_and(|m| *at_ms < m.vacated_ms) {
                            fail("moved while a crossing runway was occupied", output);
                        }
                    }
                }
                // One at a time, and separation.
                let record = &mut runways[r];
                for last in [record.last_arrival, record.last_departure] {
                    if last.is_some_and(|m| *at_ms < m.vacated_ms) {
                        fail("two aircraft on one runway", output);
                    }
                }
                if landing {
                    let occupancy = arrival_occupancy_ms(ty.wake) + config.runways[r].exit_extra_ms;
                    if *vacated_ms != at_ms + occupancy {
                        fail("wrong arrival occupancy", output);
                    }
                    if let Some(leader) = record.last_arrival
                        && *at_ms < leader.at_ms + arrival_gap(leader.ty, ty, final_nm)
                    {
                        fail("arrivals too close", output);
                    }
                    record.last_arrival = Some(movement);
                } else {
                    if *vacated_ms != at_ms + departure_occupancy_ms(ty.wake) {
                        fail("wrong departure occupancy", output);
                    }
                    if let Some(leader) = record.last_departure
                        && *at_ms < leader.at_ms + departure_separation_ms(leader.ty.wake, ty.wake)
                    {
                        fail("departures too close", output);
                    }
                    record.last_departure = Some(movement);
                }
                // Emergencies: a mayday landing shuts the runway for inspection,
                // whether or not the airport says so.
                if landing && maydays.contains(&flight.as_str()) {
                    let until = vacated_ms + config.mayday_inspection_ms;
                    record.inspection_until = record.inspection_until.max(until);
                }
            }
            Output::Rejected { .. } | Output::WindChanged { .. } => {}
        }
    }

    // No lost flights and no duplicated flights.
    for (flight, ended) in &accepted {
        if *ended != 1 {
            fail("flight did not end exactly once", &(flight, ended));
        }
    }
    if end.queue_len() != 0 {
        fail("flights left in the queue", &end.snapshot().queue);
    }
    let stats = end.stats();
    assert_eq!(stats.accepted, accepted.len() as u64, "seed {seed}");
    assert_eq!(
        stats.accepted,
        stats.landed + stats.departed + stats.diverted,
        "seed {seed}"
    );
}

fn run_and_check(seed: u64) {
    let scenario = random_scenario(seed);
    let mut airport = Airport::new(scenario.airport.clone()).unwrap();
    let mut outputs = Vec::new();
    for command in &scenario.commands {
        outputs.extend(airport.apply(command));
    }
    check(seed, &scenario, &outputs, &airport);

    // Run it twice: the same commands give the same outputs and state.
    let again = run(&scenario).unwrap();
    assert_eq!(again.outputs, outputs, "seed {seed}");
    assert_eq!(again.end, airport.snapshot(), "seed {seed}");
}

#[test]
fn no_rule_is_broken_by_random_commands() {
    // Set AIRPORT_SEEDS=first..last to try other seeds, e.g. 500..5000.
    let range = std::env::var("AIRPORT_SEEDS").ok().and_then(|v| {
        let (a, b) = v.split_once("..")?;
        Some(a.parse().ok()?..b.parse().ok()?)
    });
    for seed in range.unwrap_or(0..100) {
        run_and_check(seed);
    }
}

#[test]
fn the_demo_breaks_no_rule() {
    for seed in 0..5 {
        let scenario = airport::scenario::demo(seed, 6, 45, 45);
        let mut airport = Airport::new(scenario.airport.clone()).unwrap();
        let mut outputs = Vec::new();
        for command in &scenario.commands {
            outputs.extend(airport.apply(command));
        }
        check(seed, &scenario, &outputs, &airport);
    }
}
