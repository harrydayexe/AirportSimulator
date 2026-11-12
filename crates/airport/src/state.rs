//! The airport state machine: given the current situation and one command,
//! it produces the new situation and some outputs. It does nothing else.
//!
//! # How slots are handed out
//!
//! Waiting flights sit in one queue, ordered by emergency level, then by the
//! time they asked for, then by the order their requests arrived. Nothing is
//! decided ahead of time. Each time the clock advances, the queue is walked
//! from the front and every flight is offered the earliest moment it could
//! use a runway under the separation rules:
//!
//! - If that moment has been reached, the movement happens then, exactly,
//!   however coarse the clock steps are.
//! - If not, the flight keeps its claim on that runway. A flight further
//!   back may still use the runway first, but only if doing so does not
//!   delay the flight holding the claim.
//!
//! Because nothing is promised in advance, a closure, wind change or
//! emergency can arrive at any point and simply changes what the next walk
//! of the queue decides.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use serde::{Deserialize, Serialize};

use crate::Ms;
use crate::command::{Command, DivertReason, Emergency, Output, Reason, RejectReason};
use crate::config::{AirportConfig, ConfigError};
use crate::data::{
    MIN_OCCUPANCY_MS, aircraft, aircraft_index, arrival_occupancy_ms, departure_occupancy_ms,
    departure_separation_ms,
};
use crate::fingerprint::fingerprint;
use crate::separation::approach_gap_ms;
use crate::wind::{Wind, components};

/// Whether a flight is landing or taking off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Landing.
    Arrival,
    /// Taking off.
    Departure,
}

const PRIORITY_MAYDAY: u8 = 0;
const PRIORITY_PAN_PAN: u8 = 1;
const PRIORITY_NORMAL: u8 = 2;

fn priority(emergency: Option<Emergency>) -> u8 {
    match emergency {
        Some(Emergency::Mayday) => PRIORITY_MAYDAY,
        Some(Emergency::PanPan) => PRIORITY_PAN_PAN,
        None => PRIORITY_NORMAL,
    }
}

/// Position of a flight in the queue. Lower sorts first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct QueueKey {
    priority: u8,
    ready_ms: Ms,
    seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Flight {
    id: String,
    kind: Kind,
    /// Position in the aircraft type table.
    aircraft: u16,
    /// The flight cannot move before this.
    ready_ms: Ms,
    emergency: Option<Emergency>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FlightStatus {
    Queued(QueueKey),
    Landed,
    Departed,
    Diverted,
}

/// The last movement of one kind on a runway.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Last {
    at_ms: Ms,
    aircraft: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RunwayState {
    closures: BTreeSet<Reason>,
    /// Part of the set of runways currently in use.
    active: bool,
    /// Operating from the `high` end rather than the `low` end.
    high_end: bool,
    /// No movement before this: end of the inspection after a mayday.
    inspection_until_ms: Ms,
    last_arrival: Option<Last>,
    last_departure: Option<Last>,
}

/// Running totals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stats {
    /// Flights that joined the queue.
    pub accepted: u64,
    /// Arrivals that landed.
    pub landed: u64,
    /// Departures that took off.
    pub departed: u64,
    /// Arrivals that diverted.
    pub diverted: u64,
    /// Commands refused.
    pub rejected: u64,
    /// Sum of the delays of every flight that landed.
    pub arrival_delay_ms: Ms,
    /// Sum of the delays of every flight that departed.
    pub departure_delay_ms: Ms,
}

/// A claim on a runway by a flight near the front of the queue.
#[derive(Clone, Copy)]
struct Claim {
    at_ms: Ms,
    kind: Kind,
    aircraft: u16,
}

/// One airport. See the module documentation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Airport {
    config: AirportConfig,
    /// For each runway, a bit mask of the runways that cross it.
    crossing: Vec<u16>,
    now_ms: Ms,
    wind: Wind,
    ground_stops: BTreeSet<Reason>,
    runways: Vec<RunwayState>,
    queue: BTreeMap<QueueKey, Flight>,
    /// Every flight ever accepted, so an identifier is never used twice.
    flights: BTreeMap<String, FlightStatus>,
    next_seq: u64,
    stats: Stats,
}

impl Airport {
    /// A new airport at time zero: calm wind, every runway open, no traffic.
    ///
    /// # Errors
    /// Returns the first problem found with the configuration.
    pub fn new(config: AirportConfig) -> Result<Self, ConfigError> {
        let crossing = config.validate()?;
        let runways = config
            .runways
            .iter()
            .map(|_| RunwayState {
                closures: BTreeSet::new(),
                active: false,
                high_end: false,
                inspection_until_ms: 0,
                last_arrival: None,
                last_departure: None,
            })
            .collect();
        let mut airport = Self {
            config,
            crossing,
            now_ms: 0,
            wind: Wind::default(),
            ground_stops: BTreeSet::new(),
            runways,
            queue: BTreeMap::new(),
            flights: BTreeMap::new(),
            next_seq: 0,
            stats: Stats::default(),
        };
        airport.reconfigure(&mut Vec::new());
        Ok(airport)
    }

    /// The configuration the airport was built from.
    #[must_use]
    pub fn config(&self) -> &AirportConfig {
        &self.config
    }

    /// The current simulated time.
    #[must_use]
    pub fn now_ms(&self) -> Ms {
        self.now_ms
    }

    /// Running totals since time zero.
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Number of flights waiting for a slot.
    #[must_use]
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// The runway directions currently in use, e.g. `["27R", "27L"]`.
    #[must_use]
    pub fn active_runways(&self) -> Vec<String> {
        (0..self.runways.len())
            .filter(|r| self.runways[*r].active)
            .map(|r| self.end_name(r).to_owned())
            .collect()
    }

    /// A short hash of the entire state. Two airports given the same
    /// commands always have the same fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        fingerprint(self)
    }

    /// Applies one command and returns what the airport decided.
    pub fn apply(&mut self, command: &Command) -> Vec<Output> {
        let mut out = Vec::new();
        match command {
            Command::AdvanceTime { now_ms } => self.advance_time(*now_ms, &mut out),
            Command::RequestLanding {
                flight,
                aircraft,
                earliest_ms,
            } => self.request(flight, aircraft, *earliest_ms, Kind::Arrival, &mut out),
            Command::RequestDeparture {
                flight,
                aircraft,
                earliest_ms,
            } => self.request(flight, aircraft, *earliest_ms, Kind::Departure, &mut out),
            Command::DeclareEmergency { flight, kind } => {
                self.declare_emergency(flight, *kind, &mut out);
            }
            Command::CloseRunway { runway, reason } => {
                self.set_closure(runway, *reason, true, &mut out);
            }
            Command::OpenRunway { runway, reason } => {
                self.set_closure(runway, *reason, false, &mut out);
            }
            Command::SetWind {
                direction_deg,
                speed_kt,
            } => self.set_wind(*direction_deg, *speed_kt, &mut out),
            Command::GroundStop { reason } => {
                if self.ground_stops.insert(*reason) {
                    out.push(Output::GroundStopStarted {
                        at_ms: self.now_ms,
                        reason: *reason,
                    });
                }
            }
            Command::LiftGroundStop { reason } => {
                if self.ground_stops.remove(reason) {
                    let clear = self.ground_stops.is_empty();
                    out.push(Output::GroundStopLifted {
                        at_ms: self.now_ms,
                        reason: *reason,
                        clear,
                    });
                }
            }
        }
        out
    }

    fn reject(&mut self, subject: &str, reason: RejectReason, out: &mut Vec<Output>) {
        self.stats.rejected += 1;
        out.push(Output::Rejected {
            subject: subject.to_owned(),
            at_ms: self.now_ms,
            reason,
        });
    }

    fn advance_time(&mut self, now_ms: Ms, out: &mut Vec<Output>) {
        if now_ms < self.now_ms {
            return;
        }
        let previous = self.now_ms;
        self.now_ms = now_ms;
        self.schedule(previous, out);
        self.divert_overdue(out);
    }

    fn request(
        &mut self,
        id: &str,
        aircraft_code: &str,
        earliest_ms: Ms,
        kind: Kind,
        out: &mut Vec<Output>,
    ) {
        if self.flights.contains_key(id) {
            return self.reject(id, RejectReason::DuplicateFlight, out);
        }
        let Some(index) = aircraft_index(aircraft_code) else {
            return self.reject(id, RejectReason::UnknownAircraft, out);
        };
        let needs_m = aircraft(index).runway_m;
        if !self.config.runways.iter().any(|r| r.length_m >= needs_m) {
            return self.reject(id, RejectReason::NoSuitableRunway, out);
        }
        let flight = Flight {
            id: id.to_owned(),
            kind,
            aircraft: index,
            ready_ms: earliest_ms.max(self.now_ms),
            emergency: None,
        };
        self.enqueue(flight);
        self.stats.accepted += 1;
        out.push(Output::Accepted {
            flight: id.to_owned(),
            at_ms: self.now_ms,
        });
    }

    fn enqueue(&mut self, flight: Flight) {
        let key = QueueKey {
            priority: priority(flight.emergency),
            ready_ms: flight.ready_ms,
            seq: self.next_seq,
        };
        self.next_seq += 1;
        self.flights
            .insert(flight.id.clone(), FlightStatus::Queued(key));
        self.queue.insert(key, flight);
    }

    fn declare_emergency(&mut self, id: &str, kind: Emergency, out: &mut Vec<Output>) {
        let Some(FlightStatus::Queued(key)) = self.flights.get(id).copied() else {
            return self.reject(id, RejectReason::UnknownFlight, out);
        };
        let Some(mut flight) = self.queue.remove(&key) else {
            return;
        };
        // An emergency can be upgraded but not downgraded.
        let changed = flight.emergency.is_none_or(|current| kind > current);
        if changed {
            flight.emergency = Some(kind);
        }
        self.enqueue(flight);
        if changed {
            out.push(Output::EmergencyDeclared {
                flight: id.to_owned(),
                at_ms: self.now_ms,
                kind,
            });
        }
    }

    fn set_closure(&mut self, name: &str, reason: Reason, closed: bool, out: &mut Vec<Output>) {
        let Some(r) = self.config.runway_index(name) else {
            return self.reject(name, RejectReason::UnknownRunway, out);
        };
        let runway = self.config.runways[r].name();
        let at_ms = self.now_ms;
        let closures = &mut self.runways[r].closures;
        if closed {
            if !closures.insert(reason) {
                return;
            }
            out.push(Output::RunwayClosed {
                runway,
                at_ms,
                reason,
            });
        } else {
            if !closures.remove(&reason) {
                return;
            }
            out.push(Output::RunwayOpened {
                runway,
                at_ms,
                reason,
                open: closures.is_empty(),
            });
        }
        self.reconfigure(out);
    }

    fn set_wind(&mut self, direction_deg: u16, speed_kt: u16, out: &mut Vec<Output>) {
        if direction_deg > 360 {
            return self.reject("wind", RejectReason::InvalidWind, out);
        }
        // North is reported as either 0 or 360.
        let direction_deg = direction_deg % 360;
        let wind = Wind {
            direction_deg,
            speed_kt,
        };
        if wind == self.wind {
            return;
        }
        self.wind = wind;
        out.push(Output::WindChanged {
            at_ms: self.now_ms,
            direction_deg,
            speed_kt,
        });
        self.reconfigure(out);
    }

    fn end_name(&self, r: usize) -> &str {
        let config = &self.config.runways[r];
        if self.runways[r].high_end {
            &config.high
        } else {
            &config.low
        }
    }

    /// Chooses which runways operate together, and in which direction, for
    /// the current wind and closures.
    ///
    /// Each runway is used into the wind. A runway is a candidate if it is
    /// open and its crosswind is within its limit. A set of candidates is
    /// allowed if no two of its runways cross and it includes one of the
    /// longest candidates, because some aircraft can use nothing shorter.
    /// The largest allowed set is chosen; ties go to the set with the least
    /// total crosswind, then to the runways listed first.
    fn reconfigure(&mut self, out: &mut Vec<Output>) {
        let before = self.active_runways();
        let count = self.runways.len();
        let mut candidates: u16 = 0;
        let mut crosswind = vec![0u64; count];
        let mut high_end = vec![false; count];
        for r in 0..count {
            let config = &self.config.runways[r];
            let wind = components(self.wind, config.heading_deg);
            crosswind[r] = wind.crosswind_e4;
            high_end[r] = match wind.headwind_e4 {
                h if h > 0 => false,
                h if h < 0 => true,
                // Calm or straight across: stay as we are.
                _ => self.runways[r].high_end,
            };
            let within_limit = wind.crosswind_e4 <= u64::from(config.crosswind_limit_kt) * 10_000;
            if self.runways[r].closures.is_empty() && within_limit {
                candidates |= 1 << r;
            }
        }

        let longest_m = (0..count)
            .filter(|r| candidates & (1 << r) != 0)
            .map(|r| self.config.runways[r].length_m)
            .max();
        let mut longest: u16 = 0;
        for r in 0..count {
            if candidates & (1 << r) != 0 && Some(self.config.runways[r].length_m) == longest_m {
                longest |= 1 << r;
            }
        }

        let mut best_set: u16 = 0;
        let mut best_score = (0u32, 0u64);
        // At most `MAX_RUNWAYS` (12) runways, so every set fits in a `u16`.
        for set in 1..(1u16 << count) {
            if set & !candidates != 0 || set & longest == 0 {
                continue;
            }
            let members = (0..count).filter(|r| set & (1 << r) != 0);
            if members.clone().any(|r| self.crossing[r] & set != 0) {
                continue;
            }
            let total_crosswind: u64 = members.map(|r| crosswind[r]).sum();
            let score = (set.count_ones(), u64::MAX - total_crosswind);
            if score > best_score {
                best_score = score;
                best_set = set;
            }
        }

        for (r, runway) in self.runways.iter_mut().enumerate() {
            runway.active = best_set & (1 << r) != 0;
            runway.high_end = high_end[r];
        }
        let after = self.active_runways();
        if after != before {
            out.push(Output::ActiveRunways {
                at_ms: self.now_ms,
                runways: after,
            });
        }
    }

    /// Minimum time from one movement on runway `r` to the next: runway
    /// occupancy, or the separation between the pair, whichever is greater.
    fn gap(&self, r: usize, kind: Kind, index: u16, next_kind: Kind, next_index: u16) -> Ms {
        let leader = aircraft(index);
        let follower = aircraft(next_index);
        match kind {
            Kind::Arrival => {
                let occupancy =
                    arrival_occupancy_ms(leader.wake) + self.config.runways[r].exit_extra_ms;
                if next_kind == Kind::Arrival {
                    occupancy.max(approach_gap_ms(
                        leader,
                        follower,
                        self.config.final_approach_nm,
                    ))
                } else {
                    occupancy
                }
            }
            Kind::Departure => {
                let occupancy = departure_occupancy_ms(leader.wake);
                if next_kind == Kind::Departure {
                    occupancy.max(departure_separation_ms(leader.wake, follower.wake))
                } else {
                    occupancy
                }
            }
        }
    }

    /// When runway `r` is next clear of aircraft and inspections.
    fn busy_until(&self, r: usize) -> Ms {
        let runway = &self.runways[r];
        let mut t = runway.inspection_until_ms;
        if let Some(a) = runway.last_arrival {
            let occupancy = arrival_occupancy_ms(aircraft(a.aircraft).wake);
            t = t.max(a.at_ms + occupancy + self.config.runways[r].exit_extra_ms);
        }
        if let Some(d) = runway.last_departure {
            t = t.max(d.at_ms + departure_occupancy_ms(aircraft(d.aircraft).wake));
        }
        t
    }

    /// The earliest time `flight` could use runway `r` given everything that
    /// has already happened, or `None` if it cannot use that runway at all.
    fn runway_time(&self, r: usize, flight: &Flight, floor_ms: Ms) -> Option<Ms> {
        let runway = &self.runways[r];
        if !runway.active || self.config.runways[r].length_m < aircraft(flight.aircraft).runway_m {
            return None;
        }
        // The floor also covers every change made by a command, since all
        // of them happened at or before the previous clock time.
        let mut t = flight
            .ready_ms
            .max(floor_ms)
            .max(runway.inspection_until_ms);
        if let Some(a) = runway.last_arrival {
            let gap = self.gap(r, Kind::Arrival, a.aircraft, flight.kind, flight.aircraft);
            t = t.max(a.at_ms + gap);
        }
        if let Some(d) = runway.last_departure {
            let gap = self.gap(r, Kind::Departure, d.aircraft, flight.kind, flight.aircraft);
            t = t.max(d.at_ms + gap);
        }
        // A crossing runway is never active at the same time, but its last
        // aircraft may still be on it just after a change of runways.
        for c in 0..self.runways.len() {
            if self.crossing[r] & (1 << c) != 0 {
                t = t.max(self.busy_until(c));
            }
        }
        Some(t)
    }

    /// Whether no flight could fit on runway `r` ahead of a claim at `claim_ms`.
    fn sealed(&self, r: usize, claim_ms: Ms, floor_ms: Ms) -> bool {
        self.busy_until(r).max(floor_ms) + MIN_OCCUPANCY_MS > claim_ms
    }

    /// Walks the queue and performs every movement whose time has come.
    /// `floor_ms` is the previous clock time: nothing is decided now for a
    /// moment before it.
    fn schedule(&mut self, floor_ms: Ms, out: &mut Vec<Output>) {
        if !self.ground_stops.is_empty() {
            return;
        }
        let count = self.runways.len();
        let mut claims: Vec<Option<Claim>> = vec![None; count];
        // Runways on which a flight further back might still fit.
        let mut open = self.runways.iter().filter(|r| r.active).count();
        let mut cursor: Option<QueueKey> = None;
        while open > 0 {
            let next = match cursor {
                None => self.queue.iter().next(),
                Some(key) => self
                    .queue
                    .range((Bound::Excluded(key), Bound::Unbounded))
                    .next(),
            };
            let Some((&key, flight)) = next else { break };
            cursor = Some(key);
            if key.priority == PRIORITY_NORMAL && key.ready_ms > self.now_ms {
                // Everything from here on is not ready yet either.
                break;
            }

            let mut best: Option<(usize, Ms)> = None;
            for (r, claim) in claims.iter().enumerate() {
                let Some(mut t) = self.runway_time(r, flight, floor_ms) else {
                    continue;
                };
                if let Some(claim) = *claim {
                    // Going first is allowed only if the claim is not delayed.
                    if t + self.gap_behind(r, flight, claim) > claim.at_ms {
                        let gap =
                            self.gap(r, claim.kind, claim.aircraft, flight.kind, flight.aircraft);
                        t = t.max(claim.at_ms + gap);
                    }
                }
                if best.is_none_or(|(_, best_t)| t < best_t) {
                    best = Some((r, t));
                }
            }
            let Some((r, t)) = best else { continue };
            if self.hold_deadline(flight).is_some_and(|d| t > d) {
                // It will divert before it could land: it claims nothing.
                continue;
            }
            if t > self.now_ms {
                if claims[r].is_none() {
                    claims[r] = Some(Claim {
                        at_ms: t,
                        kind: flight.kind,
                        aircraft: flight.aircraft,
                    });
                    if self.sealed(r, t, floor_ms) {
                        open -= 1;
                    }
                }
                continue;
            }

            let Some(flight) = self.queue.remove(&key) else {
                break;
            };
            self.perform(&flight, r, t, out);
            if let Some(claim) = claims[r]
                && self.sealed(r, claim.at_ms, floor_ms)
            {
                open -= 1;
            }
        }
    }

    /// Minimum time from `flight` using runway `r` to the claimed movement.
    fn gap_behind(&self, r: usize, flight: &Flight, claim: Claim) -> Ms {
        let gap = self.gap(r, flight.kind, flight.aircraft, claim.kind, claim.aircraft);
        if flight.kind == Kind::Arrival && flight.emergency == Some(Emergency::Mayday) {
            let occupancy = arrival_occupancy_ms(aircraft(flight.aircraft).wake)
                + self.config.runways[r].exit_extra_ms;
            gap.max(occupancy + self.config.mayday_inspection_ms)
        } else {
            gap
        }
    }

    /// The moment an ordinary arrival gives up waiting, if there is one.
    fn hold_deadline(&self, flight: &Flight) -> Option<Ms> {
        if flight.kind == Kind::Arrival && flight.emergency.is_none() {
            self.config
                .max_arrival_hold_ms
                .map(|hold| flight.ready_ms + hold)
        } else {
            None
        }
    }

    /// Records the movement of `flight` on runway `r` at `at_ms`.
    fn perform(&mut self, flight: &Flight, r: usize, at_ms: Ms, out: &mut Vec<Output>) {
        let ty = aircraft(flight.aircraft);
        let runway = self.end_name(r).to_owned();
        let delay_ms = at_ms - flight.ready_ms;
        let last = Some(Last {
            at_ms,
            aircraft: flight.aircraft,
        });
        match flight.kind {
            Kind::Arrival => {
                let vacated_ms =
                    at_ms + arrival_occupancy_ms(ty.wake) + self.config.runways[r].exit_extra_ms;
                self.runways[r].last_arrival = last;
                self.flights.insert(flight.id.clone(), FlightStatus::Landed);
                self.stats.landed += 1;
                self.stats.arrival_delay_ms += delay_ms;
                out.push(Output::Landed {
                    flight: flight.id.clone(),
                    aircraft: ty.icao.to_owned(),
                    runway,
                    at_ms,
                    vacated_ms,
                    delay_ms,
                    emergency: flight.emergency,
                });
                let inspection = self.config.mayday_inspection_ms;
                if flight.emergency == Some(Emergency::Mayday) && inspection > 0 {
                    let until_ms = vacated_ms + inspection;
                    self.runways[r].inspection_until_ms = until_ms;
                    out.push(Output::RunwayInspection {
                        runway: self.config.runways[r].name(),
                        from_ms: vacated_ms,
                        until_ms,
                    });
                }
            }
            Kind::Departure => {
                self.runways[r].last_departure = last;
                self.flights
                    .insert(flight.id.clone(), FlightStatus::Departed);
                self.stats.departed += 1;
                self.stats.departure_delay_ms += delay_ms;
                out.push(Output::Departed {
                    flight: flight.id.clone(),
                    aircraft: ty.icao.to_owned(),
                    runway,
                    at_ms,
                    vacated_ms: at_ms + departure_occupancy_ms(ty.wake),
                    delay_ms,
                    emergency: flight.emergency,
                });
            }
        }
    }

    /// Diverts every ordinary arrival that has run out of holding time.
    fn divert_overdue(&mut self, out: &mut Vec<Output>) {
        let Some(hold) = self.config.max_arrival_hold_ms else {
            return;
        };
        let first_normal = QueueKey {
            priority: PRIORITY_NORMAL,
            ready_ms: 0,
            seq: 0,
        };
        let overdue: Vec<QueueKey> = self
            .queue
            .range(first_normal..)
            .take_while(|(key, _)| key.ready_ms + hold <= self.now_ms)
            .filter(|(_, flight)| flight.kind == Kind::Arrival)
            .map(|(key, _)| *key)
            .collect();
        for key in overdue {
            let Some(flight) = self.queue.remove(&key) else {
                continue;
            };
            self.stats.diverted += 1;
            out.push(Output::Diverted {
                at_ms: flight.ready_ms + hold,
                reason: DivertReason::HoldLimit,
                flight: flight.id.clone(),
            });
            self.flights.insert(flight.id, FlightStatus::Diverted);
        }
    }

    /// A description of the current state for display.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            now_ms: self.now_ms,
            wind: self.wind,
            ground_stops: self.ground_stops.iter().copied().collect(),
            runways: (0..self.runways.len())
                .map(|r| {
                    let runway = &self.runways[r];
                    RunwaySnapshot {
                        name: self.config.runways[r].name(),
                        active_end: runway.active.then(|| self.end_name(r).to_owned()),
                        closures: runway.closures.iter().copied().collect(),
                        inspection_until_ms: (runway.inspection_until_ms > self.now_ms)
                            .then_some(runway.inspection_until_ms),
                    }
                })
                .collect(),
            queue: self
                .queue
                .values()
                .map(|flight| QueuedFlight {
                    flight: flight.id.clone(),
                    kind: flight.kind,
                    aircraft: aircraft(flight.aircraft).icao.to_owned(),
                    ready_ms: flight.ready_ms,
                    emergency: flight.emergency,
                })
                .collect(),
            stats: self.stats,
            fingerprint: format!("{:016x}", self.fingerprint()),
        }
    }
}

/// See [`Airport::snapshot`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The current simulated time.
    pub now_ms: Ms,
    /// The current wind.
    pub wind: Wind,
    /// Reasons for the ground stops in force; empty when movements may run.
    pub ground_stops: Vec<Reason>,
    /// Every runway, in configuration order.
    pub runways: Vec<RunwaySnapshot>,
    /// Waiting flights, front of the queue first.
    pub queue: Vec<QueuedFlight>,
    /// Running totals since time zero.
    pub stats: Stats,
    /// The state fingerprint as 16 hexadecimal digits.
    pub fingerprint: String,
}

/// One runway in a [`Snapshot`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunwaySnapshot {
    /// The full name, e.g. `09L/27R`.
    pub name: String,
    /// The direction in use, or `None` if the runway is not in use.
    pub active_end: Option<String>,
    /// Reasons the runway is closed; empty when it is open.
    pub closures: Vec<Reason>,
    /// End of a mayday inspection still under way, if any.
    pub inspection_until_ms: Option<Ms>,
}

/// One waiting flight in a [`Snapshot`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedFlight {
    /// The flight identifier.
    pub flight: String,
    /// Landing or taking off.
    pub kind: Kind,
    /// ICAO type designator, e.g. `A320`.
    pub aircraft: String,
    /// The flight cannot move before this.
    pub ready_ms: Ms,
    /// The emergency it has declared, if any.
    pub emergency: Option<Emergency>,
}
