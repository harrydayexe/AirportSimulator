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
//! - **Emergencies first.** No ordinary flight takes a slot that delays a
//!   declared emergency on the same runway.
//! - **Nobody moves before they asked**, an ordinary arrival never holds past
//!   its limit, and outputs are never dated after the clock.
//! - **Inspections** follow exactly the mayday landings that need them.
//! - **Totals** in the stats match the outputs.
//! - **No lost flights, no duplicated flights.** Every accepted flight ends
//!   exactly once: landed, departed or diverted.
//!
//! The checker shares the data tables with the airport but none of its
//! logic. A failure prints its seed; rerun with that seed to replay it.
//!
//! The airport's own whole-state scan (`Airport::check_invariants`, A1 to
//! A12) also runs after every command here, because the tests build the
//! crate with the `paranoid` feature.

use std::collections::BTreeMap;

use airport::command::DivertReason;
use airport::data::{
    AIRCRAFT_TYPES, AircraftType, MAX_TIME_MS, aircraft, aircraft_index, approach_separation_nm,
    arrival_occupancy_ms, departure_occupancy_ms, departure_separation_ms,
};
use airport::rng::Rng;
use airport::scenario::{Scenario, Timed, run, script};
use airport::{
    Airport, AirportConfig, Command, Emergency, MS_PER_HOUR, MS_PER_MINUTE, Ms, Output, Reason,
    RunwayConfig, policy,
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
            // Up to the most an airport may have, so `reconfigure` searches
            // the largest sets it ever will.
            let count = 1 + rng.below(12);
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

/// A name the airport has never heard of, now and then empty or very long.
fn unknown_name(rng: &mut Rng) -> String {
    match rng.below(3) {
        0 => String::new(),
        1 => "Z".repeat(1_000),
        _ => "NOBODY".to_owned(),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one generator with every kind of input is easier to follow in one place"
)]
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
        let command = match rng.below(10) {
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
            8 if !flights.is_empty() => {
                let (flight, filed_ms) = &flights[index(&mut rng, flights.len())];
                let declared_ms = filed_ms + rng.below(40 * MS_PER_MINUTE);
                let kinds = [Emergency::PanPan, Emergency::Mayday];
                timed.push(Timed {
                    at_ms: declared_ms,
                    command: Command::DeclareEmergency {
                        flight: flight.clone(),
                        kind: kinds[index(&mut rng, 2)],
                    },
                });
                // Sometimes declared again: a repeat, an upgrade or a
                // downgrade.
                if rng.below(3) == 0 {
                    timed.push(Timed {
                        at_ms: declared_ms + rng.below(10 * MS_PER_MINUTE),
                        command: Command::DeclareEmergency {
                            flight: flight.clone(),
                            kind: kinds[index(&mut rng, 2)],
                        },
                    });
                }
                continue;
            }
            // Commands the airport must refuse.
            _ => match rng.below(5) {
                0 => Command::RequestLanding {
                    flight: format!("X{}", rng.below(1_000)),
                    aircraft: unknown_name(&mut rng),
                    earliest_ms: at_ms,
                },
                1 => Command::DeclareEmergency {
                    flight: unknown_name(&mut rng),
                    kind: Emergency::Mayday,
                },
                2 => Command::CloseRunway {
                    runway: unknown_name(&mut rng),
                    reason,
                },
                3 => Command::OpenRunway {
                    runway: unknown_name(&mut rng),
                    reason,
                },
                _ => Command::SetWind {
                    direction_deg: 361 + below::<u16>(&mut rng, u64::from(u16::MAX - 361)),
                    speed_kt: below(&mut rng, 45),
                },
            },
        };
        timed.push(Timed { at_ms, command });
    }

    // The schedule generators from `policy`, now and then.
    if rng.below(4) == 0 {
        let start = rng.below(24) * MS_PER_HOUR;
        let end = rng.below(24) * MS_PER_HOUR;
        timed.extend(policy::curfew(start, end, duration));
    }
    if rng.below(4) == 0 {
        let runway = &airport.runways[index(&mut rng, airport.runways.len())];
        let length = (1 + rng.below(60)) * MS_PER_MINUTE;
        let every = length + rng.below(2 * MS_PER_HOUR);
        timed.extend(policy::maintenance(
            &runway.name(),
            rng.below(duration),
            length,
            every,
            duration,
        ));
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
    let mut commands = script(timed, tick_ms, clear_ms + 30 * MS_PER_HOUR);

    // `script` only ever moves time forward. Drop in a few clocks that go
    // backwards, which the airport must ignore.
    for _ in 0..rng.below(5) {
        let at = index(&mut rng, commands.len() + 1);
        let now_ms = rng.below(clear_ms);
        commands.insert(at, Command::AdvanceTime { now_ms });
    }

    Scenario { airport, commands }
}

#[derive(Clone, Copy)]
struct Moved {
    landing: bool,
    at_ms: Ms,
    vacated_ms: Ms,
    ty: &'static AircraftType,
    /// Whether the flight had declared no emergency.
    ordinary: bool,
    /// Position of the command that produced the movement.
    step: usize,
}

#[derive(Default)]
struct RunwayRecord {
    /// Every movement on the runway, in order.
    history: Vec<Moved>,
    /// When the runway last became usable, or `None` while it is closed.
    open_since: Option<Ms>,
    inspection_until: Ms,
}

impl RunwayRecord {
    fn last(movements: &[Moved], landing: bool) -> Option<Moved> {
        movements
            .iter()
            .rev()
            .find(|m| m.landing == landing)
            .copied()
    }
}

struct FlightRecord {
    landing: bool,
    /// The earliest the flight may move: the time it asked for, or the
    /// time it asked if that was later.
    ready_ms: Ms,
    /// The clock and the step when it first declared an emergency.
    declared: Option<(Ms, usize)>,
    /// The most serious emergency it has declared.
    emergency: Option<Emergency>,
    ended: u32,
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

/// Minimum time from `leader` using runway `r` to the next movement.
fn gap_after(
    config: &AirportConfig,
    r: usize,
    leader: &Moved,
    landing: bool,
    ty: &AircraftType,
) -> Ms {
    let final_nm = u64::from(config.final_approach_nm);
    let occupancy = if leader.landing {
        arrival_occupancy_ms(leader.ty.wake) + config.runways[r].exit_extra_ms
    } else {
        departure_occupancy_ms(leader.ty.wake)
    };
    let separation = match (leader.landing, landing) {
        (true, true) => arrival_gap(leader.ty, ty, final_nm),
        (false, false) => departure_separation_ms(leader.ty.wake, ty.wake),
        _ => 0,
    };
    occupancy.max(separation)
}

/// Every command a scenario sent, with the outputs it produced.
type Steps<'a> = [(&'a Command, Vec<Output>)];

/// Checks every rule over `steps`. With `complete` false the run was cut
/// short, so flights may still be waiting.
#[expect(
    clippy::too_many_lines,
    reason = "one pass over the outputs checks every rule; splitting it scatters the shared state"
)]
fn check(seed: u64, config: &AirportConfig, steps: &Steps, end: &Airport, complete: bool) {
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
    let mut flights: BTreeMap<&str, FlightRecord> = BTreeMap::new();
    let mut clock: Ms = 0;
    // The last step that changed which runways may be used, or how.
    let mut last_disruption: Option<usize> = None;
    let mut rejected = 0u64;
    let (mut arrival_delay, mut departure_delay) = (0, 0);

    for (step, (command, outputs)) in steps.iter().enumerate() {
        let clock_before = clock;
        let refused = outputs.iter().any(|o| matches!(o, Output::Rejected { .. }));
        if let Command::AdvanceTime { now_ms } = command
            && !refused
        {
            clock = clock.max(*now_ms);
        }
        // A mayday landing's inspection must come straight after it.
        let mut pending_inspection: Option<(usize, Ms, Ms)> = None;

        for output in outputs {
            if let Some(expected) = pending_inspection.take() {
                match output {
                    Output::RunwayInspection {
                        runway,
                        from_ms,
                        until_ms,
                    } if (config.runway_index(runway).unwrap(), *from_ms, *until_ms)
                        == expected =>
                    {
                        continue;
                    }
                    _ => fail("mayday landing without its inspection", output),
                }
            }

            // Outputs are not from the future.
            let at_ms = match output {
                Output::RunwayInspection { .. } => None,
                Output::Accepted { at_ms, .. }
                | Output::Rejected { at_ms, .. }
                | Output::Landed { at_ms, .. }
                | Output::Departed { at_ms, .. }
                | Output::Diverted { at_ms, .. }
                | Output::EmergencyDeclared { at_ms, .. }
                | Output::RunwayClosed { at_ms, .. }
                | Output::RunwayOpened { at_ms, .. }
                | Output::ActiveRunways { at_ms, .. }
                | Output::WindChanged { at_ms, .. }
                | Output::GroundStopStarted { at_ms, .. }
                | Output::GroundStopLifted { at_ms, .. } => Some(*at_ms),
            };
            if at_ms.is_some_and(|t| t > clock) {
                fail("output dated after the clock", output);
            }

            match output {
                Output::Accepted { flight, .. } => {
                    let (landing, earliest_ms) = match command {
                        Command::RequestLanding { earliest_ms, .. } => (true, *earliest_ms),
                        Command::RequestDeparture { earliest_ms, .. } => (false, *earliest_ms),
                        _ => fail("accepted something that is not a request", output),
                    };
                    let record = FlightRecord {
                        landing,
                        ready_ms: earliest_ms.max(clock),
                        declared: None,
                        emergency: None,
                        ended: 0,
                    };
                    if flights.insert(flight, record).is_some() {
                        fail("flight accepted twice", output);
                    }
                }
                Output::Rejected { .. } => rejected += 1,
                Output::EmergencyDeclared {
                    flight,
                    kind,
                    at_ms,
                } => {
                    let Some(record) = flights.get_mut(flight.as_str()) else {
                        fail("emergency for an unknown flight", output)
                    };
                    if record.ended != 0 {
                        fail("emergency after the flight ended", output);
                    }
                    if record.emergency.is_some_and(|e| *kind <= e) {
                        fail("emergency repeated or downgraded", output);
                    }
                    record.emergency = Some(*kind);
                    record.declared.get_or_insert((*at_ms, step));
                }
                Output::Diverted {
                    flight,
                    reason,
                    at_ms,
                } => {
                    let DivertReason::HoldLimit = reason;
                    let Some(hold) = config.max_arrival_hold_ms else {
                        fail("diverted with no hold limit", output)
                    };
                    let Some(record) = flights.get_mut(flight.as_str()) else {
                        fail("unknown flight diverted", output)
                    };
                    record.ended += 1;
                    if !record.landing {
                        fail("a departure diverted", output);
                    }
                    if record.emergency.is_some() {
                        fail("an emergency diverted", output);
                    }
                    if *at_ms != record.ready_ms + hold {
                        fail("diverted at the wrong time", output);
                    }
                }
                Output::RunwayClosed { runway, .. } => {
                    let r = config.runway_index(runway).unwrap();
                    runways[r].open_since = None;
                    last_disruption = Some(step);
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
                    last_disruption = Some(step);
                }
                Output::RunwayInspection { .. } => {
                    fail("inspection with no mayday landing", output);
                }
                Output::ActiveRunways { runways, .. } => {
                    active.clone_from(runways);
                    last_disruption = Some(step);
                }
                Output::GroundStopStarted { .. } => {
                    ground_stopped = true;
                    last_disruption = Some(step);
                }
                Output::GroundStopLifted { at_ms, clear, .. } => {
                    if *clear {
                        ground_stopped = false;
                        ground_clear_since = *at_ms;
                    }
                    last_disruption = Some(step);
                }
                Output::WindChanged { .. } => last_disruption = Some(step),
                Output::Landed {
                    flight,
                    aircraft: code,
                    runway,
                    at_ms,
                    vacated_ms,
                    delay_ms,
                    emergency,
                }
                | Output::Departed {
                    flight,
                    aircraft: code,
                    runway,
                    at_ms,
                    vacated_ms,
                    delay_ms,
                    emergency,
                } => {
                    let landing = matches!(output, Output::Landed { .. });
                    let ty = aircraft(aircraft_index(code).unwrap());
                    let r = config.runway_index(runway).unwrap();
                    let movement = Moved {
                        landing,
                        at_ms: *at_ms,
                        vacated_ms: *vacated_ms,
                        ty,
                        ordinary: emergency.is_none(),
                        step,
                    };

                    let Some(flight_record) = flights.get_mut(flight.as_str()) else {
                        fail("unknown flight moved", output)
                    };
                    flight_record.ended += 1;
                    if flight_record.landing != landing {
                        fail("moved the wrong way", output);
                    }
                    if *emergency != flight_record.emergency {
                        fail("wrong emergency reported", output);
                    }
                    // Nobody moves before they asked, and the delay is
                    // measured from then.
                    let ready_ms = flight_record.ready_ms;
                    if *at_ms < ready_ms || *delay_ms != at_ms - ready_ms {
                        fail("moved before it was ready, or wrong delay", output);
                    }
                    if *at_ms < clock_before {
                        fail("movement decided in the past", output);
                    }
                    // Hold limit.
                    if landing
                        && emergency.is_none()
                        && config
                            .max_arrival_hold_ms
                            .is_some_and(|hold| *delay_ms > hold)
                    {
                        fail("held past the limit", output);
                    }
                    if landing {
                        arrival_delay += delay_ms;
                    } else {
                        departure_delay += delay_ms;
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
                    let mut crossing_busy: Ms = 0;
                    for (c, other) in runways.iter().enumerate() {
                        if crossing[r] & (1 << c) == 0 {
                            continue;
                        }
                        crossing_busy = crossing_busy.max(other.inspection_until);
                        for last in [
                            RunwayRecord::last(&other.history, true),
                            RunwayRecord::last(&other.history, false),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            crossing_busy = crossing_busy.max(last.vacated_ms);
                            if *at_ms < last.vacated_ms {
                                fail("moved while a crossing runway was occupied", output);
                            }
                        }
                    }
                    // One at a time, and separation.
                    let record = &mut runways[r];
                    for last in [
                        RunwayRecord::last(&record.history, true),
                        RunwayRecord::last(&record.history, false),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if *at_ms < last.vacated_ms {
                            fail("two aircraft on one runway", output);
                        }
                    }
                    if landing {
                        let occupancy =
                            arrival_occupancy_ms(ty.wake) + config.runways[r].exit_extra_ms;
                        if *vacated_ms != at_ms + occupancy {
                            fail("wrong arrival occupancy", output);
                        }
                        if let Some(leader) = RunwayRecord::last(&record.history, true)
                            && *at_ms < leader.at_ms + arrival_gap(leader.ty, ty, final_nm)
                        {
                            fail("arrivals too close", output);
                        }
                    } else {
                        if *vacated_ms != at_ms + departure_occupancy_ms(ty.wake) {
                            fail("wrong departure occupancy", output);
                        }
                        if let Some(leader) = RunwayRecord::last(&record.history, false)
                            && *at_ms
                                < leader.at_ms + departure_separation_ms(leader.ty.wake, ty.wake)
                        {
                            fail("departures too close", output);
                        }
                    }

                    // Emergencies first. If the movement just before this
                    // emergency was an ordinary flight that went after the
                    // emergency was declared, it must not have delayed it:
                    // the emergency must go no later than it could have
                    // without that flight. Only checked when nothing else
                    // about the runways changed in between, so that the
                    // bound below is all that could have held it back.
                    if !movement.ordinary
                        && let Some((declared_ms, declared_step)) = flight_record.declared
                        && let Some((previous, before)) = record.history.split_last()
                        && previous.ordinary
                        && previous.step > declared_step
                        && last_disruption.is_none_or(|d| d < declared_step)
                    {
                        let mut earliest = ready_ms
                            .max(declared_ms)
                            .max(ground_clear_since)
                            .max(record.open_since.unwrap_or(0))
                            .max(record.inspection_until)
                            .max(crossing_busy);
                        for leader in [
                            RunwayRecord::last(before, true),
                            RunwayRecord::last(before, false),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            earliest = earliest
                                .max(leader.at_ms + gap_after(config, r, &leader, landing, ty));
                        }
                        if *at_ms > earliest {
                            fail(
                                "an ordinary flight delayed an emergency",
                                &(output, previous.at_ms, earliest),
                            );
                        }
                    }

                    record.history.push(movement);
                    // A mayday landing shuts the runway for inspection.
                    if landing
                        && *emergency == Some(Emergency::Mayday)
                        && config.mayday_inspection_ms > 0
                    {
                        let until = vacated_ms + config.mayday_inspection_ms;
                        record.inspection_until = record.inspection_until.max(until);
                        pending_inspection = Some((r, *vacated_ms, until));
                    }
                }
            }
        }
        if pending_inspection.is_some() {
            fail("mayday landing without its inspection", command);
        }
    }

    // Totals, at whatever point the run stopped.
    let stats = end.stats();
    let count = |landing: bool| -> u64 {
        runways
            .iter()
            .flat_map(|r| &r.history)
            .filter(|m| m.landing == landing)
            .count() as u64
    };
    assert_eq!(stats.landed, count(true), "seed {seed}: landed");
    assert_eq!(stats.departed, count(false), "seed {seed}: departed");
    assert_eq!(
        stats.arrival_delay_ms, arrival_delay,
        "seed {seed}: arrival delay"
    );
    assert_eq!(
        stats.departure_delay_ms, departure_delay,
        "seed {seed}: departure delay"
    );
    assert_eq!(stats.rejected, rejected, "seed {seed}: rejected");
    assert_eq!(
        stats.accepted,
        flights.len() as u64,
        "seed {seed}: accepted"
    );

    // No duplicated flights, and once the run is complete, no lost ones.
    for (flight, record) in &flights {
        let ended = record.ended;
        if ended > 1 || (complete && ended != 1) {
            fail("flight did not end exactly once", &(flight, ended));
        }
    }
    if complete {
        if end.queue_len() != 0 {
            fail("flights left in the queue", &end.snapshot().queue);
        }
        assert_eq!(
            stats.accepted,
            stats.landed + stats.departed + stats.diverted,
            "seed {seed}"
        );
    }
}

/// Runs `commands` and returns each with its outputs. A refusal must change
/// nothing but the count of refusals: the state after it equals the state
/// before plus one other refusal.
fn run_steps<'a>(
    airport: &mut Airport,
    commands: &'a [Command],
) -> Vec<(&'a Command, Vec<Output>)> {
    let refuse = Command::SetWind {
        direction_deg: 361,
        speed_kt: 0,
    };
    commands
        .iter()
        .map(|command| {
            // Cloning before every tick would be slow, and a tick is only
            // refused past the time cap, which the scenarios never reach.
            let before = (!matches!(command, Command::AdvanceTime { .. })).then(|| airport.clone());
            let outputs = airport.apply(command);
            if let Some(mut before) = before
                && matches!(outputs[..], [Output::Rejected { .. }])
            {
                before.apply(&refuse);
                assert!(
                    before == *airport,
                    "{command:?} was refused but changed the state"
                );
            }
            (command, outputs)
        })
        .collect()
}

fn run_and_check(seed: u64) {
    let scenario = random_scenario(seed);
    let mut airport = Airport::new(scenario.airport.clone()).unwrap();
    let steps = run_steps(&mut airport, &scenario.commands);
    check(seed, &scenario.airport, &steps, &airport, true);

    // Run it twice: the same commands give the same outputs and state.
    let outputs: Vec<Output> = steps.into_iter().flat_map(|(_, o)| o).collect();
    let again = run(&scenario).unwrap();
    assert_eq!(again.outputs, outputs, "seed {seed}");
    assert_eq!(again.end, airport.snapshot(), "seed {seed}");
}

/// The seeds to try: `AIRPORT_SEEDS=first..last` if set, otherwise `0..100`.
#[expect(
    clippy::disallowed_methods,
    reason = "the seed range is test configuration, not simulation input"
)]
fn seeds() -> std::ops::Range<u64> {
    let range = std::env::var("AIRPORT_SEEDS").ok().and_then(|v| {
        let (a, b) = v.split_once("..")?;
        Some(a.parse().ok()?..b.parse().ok()?)
    });
    range.unwrap_or(0..100)
}

#[test]
fn no_rule_is_broken_by_random_commands() {
    // Set AIRPORT_SEEDS=first..last to try other seeds, e.g. 500..5000.
    for seed in seeds() {
        run_and_check(seed);
    }
}

#[test]
fn no_rule_is_broken_at_any_point_in_a_run() {
    // The drain at the end could hide a rule that is broken only for a
    // while. Every rule but "no lost flights" holds at any cut, so stop
    // each run at a random point and check there.
    for seed in seeds() {
        let scenario = random_scenario(seed);
        let cut = index(&mut Rng::new(seed ^ 0xC07), scenario.commands.len() + 1);
        let mut airport = Airport::new(scenario.airport.clone()).unwrap();
        let steps = run_steps(&mut airport, &scenario.commands[..cut]);
        airport.check_invariants();
        check(seed, &scenario.airport, &steps, &airport, false);
    }
}

#[test]
fn the_demo_breaks_no_rule() {
    for seed in 0..5 {
        let scenario = airport::scenario::demo(seed, 6, 45, 45);
        let mut airport = Airport::new(scenario.airport.clone()).unwrap();
        let steps = run_steps(&mut airport, &scenario.commands);
        check(seed, &scenario.airport, &steps, &airport, true);
    }
}

#[test]
fn time_in_a_script_only_moves_forward() {
    // Before the backward clocks are dropped in, as `script` builds it.
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        let timed: Vec<Timed> = (0..50)
            .map(|_| Timed {
                at_ms: rng.below(10 * MS_PER_HOUR),
                command: Command::GroundStop {
                    reason: Reason::Other,
                },
            })
            .collect();
        let mut sent: Vec<Ms> = timed.iter().map(|t| t.at_ms).collect();
        sent.sort_unstable();
        let tick_ms = 1 + rng.below(MS_PER_HOUR);
        let commands = script(timed, tick_ms, 12 * MS_PER_HOUR);
        let mut clock: Option<Ms> = None;
        let mut sent = sent.into_iter();
        for command in &commands {
            match command {
                Command::AdvanceTime { now_ms } => {
                    assert!(clock.is_none_or(|c| *now_ms > c), "seed {seed}");
                    clock = Some(*now_ms);
                }
                _ => assert_eq!(Some(clock.unwrap_or(0)), sent.next(), "seed {seed}"),
            }
        }
        assert_eq!(sent.next(), None, "seed {seed}");
    }
}

/// A random value anywhere in `u64`, biased towards the edges.
fn extreme(rng: &mut Rng) -> Ms {
    match rng.below(6) {
        0 => 0,
        1 => u64::MAX,
        2 => u64::MAX - rng.below(1_000),
        3 => MAX_TIME_MS - 1 + rng.below(3),
        4 => rng.below(10 * MS_PER_HOUR),
        _ => rng.next_u64(),
    }
}

/// A name that may or may not be one the airport knows.
fn some_name(rng: &mut Rng, known: &[String]) -> String {
    if known.is_empty() || rng.below(3) == 0 {
        unknown_name(rng)
    } else {
        known[index(rng, known.len())].clone()
    }
}

#[test]
fn no_command_can_crash_the_airport() {
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        let config = random_airport(&mut rng);
        let runway_names: Vec<String> = config
            .runways
            .iter()
            .flat_map(|r| [r.low.clone(), r.high.clone(), r.name()])
            .collect();
        let codes: Vec<String> = AIRCRAFT_TYPES.iter().map(|t| t.icao.to_owned()).collect();
        let mut airport = Airport::new(config).unwrap();
        let mut flights: Vec<String> = Vec::new();
        for _ in 0..300 {
            let reason = REASONS[index(&mut rng, REASONS.len())];
            let command = match rng.below(8) {
                0 | 1 => {
                    let flight = some_name(&mut rng, &flights);
                    let flight = if flight == "NOBODY" {
                        format!("F{}", flights.len())
                    } else {
                        flight
                    };
                    flights.push(flight.clone());
                    let aircraft = some_name(&mut rng, &codes);
                    let earliest_ms = extreme(&mut rng);
                    if rng.below(2) == 0 {
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
                    }
                }
                2 => Command::AdvanceTime {
                    now_ms: extreme(&mut rng),
                },
                3 => Command::DeclareEmergency {
                    flight: some_name(&mut rng, &flights),
                    kind: [Emergency::PanPan, Emergency::Mayday][index(&mut rng, 2)],
                },
                4 => Command::CloseRunway {
                    runway: some_name(&mut rng, &runway_names),
                    reason,
                },
                5 => Command::OpenRunway {
                    runway: some_name(&mut rng, &runway_names),
                    reason,
                },
                6 => Command::SetWind {
                    direction_deg: below(&mut rng, u64::from(u16::MAX) + 1),
                    speed_kt: below(&mut rng, u64::from(u16::MAX) + 1),
                },
                _ if rng.below(2) == 0 => Command::GroundStop { reason },
                _ => Command::LiftGroundStop { reason },
            };
            let unknown_aircraft = matches!(
                &command,
                Command::RequestLanding { aircraft, .. } | Command::RequestDeparture { aircraft, .. }
                    if aircraft_index(aircraft).is_none()
            );
            let outputs = airport.apply(&command);
            if unknown_aircraft {
                assert!(
                    matches!(outputs[..], [Output::Rejected { .. }]),
                    "seed {seed}: {command:?} gave {outputs:?}"
                );
            }
        }
        airport.check_invariants();
    }
}
