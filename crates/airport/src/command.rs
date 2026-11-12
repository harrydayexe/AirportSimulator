//! What goes into an airport (commands) and what comes out (outputs).
//!
//! Every time is simulated milliseconds ([`Ms`]). A flight is named by its
//! identifier (e.g. `BAW123`), an aircraft by its ICAO type designator (e.g.
//! `A320`).

use serde::{Deserialize, Serialize};

use crate::Ms;

/// Why a runway is closed or movements are stopped. Closures and ground
/// stops are tracked per reason, so a storm closure and a maintenance
/// closure of the same runway can overlap and be lifted separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Planned work on the runway or airfield.
    Maintenance,
    /// Weather unsafe for movements.
    Storm,
    /// Night-time noise restrictions.
    Curfew,
    /// An accident or obstruction.
    Incident,
    /// Any other reason.
    Other,
}

/// The two levels of declared emergency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emergency {
    /// Urgent.
    PanPan,
    /// Life-threatening.
    Mayday,
}

/// One instruction or fact sent to an airport.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// "The simulated time is now X." Time never goes backwards: an earlier
    /// time is ignored.
    AdvanceTime {
        /// The new simulated time.
        now_ms: Ms,
    },
    /// A flight wants to land, no earlier than `earliest_ms`.
    RequestLanding {
        /// The flight identifier; must not have been used before.
        flight: String,
        /// ICAO type designator, e.g. `A320`.
        aircraft: String,
        /// The earliest time the flight can be at the runway.
        earliest_ms: Ms,
    },
    /// A flight wants to take off, no earlier than `earliest_ms`.
    RequestDeparture {
        /// The flight identifier; must not have been used before.
        flight: String,
        /// ICAO type designator, e.g. `A320`.
        aircraft: String,
        /// The earliest time the flight can be ready to roll.
        earliest_ms: Ms,
    },
    /// A waiting flight has declared an emergency.
    DeclareEmergency {
        /// The waiting flight.
        flight: String,
        /// How serious the emergency is.
        kind: Emergency,
    },
    /// A runway is unusable until opened again. `runway` is either end's
    /// name (`27L`) or the full name (`09R/27L`).
    CloseRunway {
        /// The runway to close.
        runway: String,
        /// Why it is closed; opening it again names the same reason.
        reason: Reason,
    },
    /// Lifts the closure made for `reason`. The runway is usable again once
    /// no closure remains.
    OpenRunway {
        /// The runway, named as in `CloseRunway`.
        runway: String,
        /// The closure to lift.
        reason: Reason,
    },
    /// The wind is now this: the direction it blows from in degrees true
    /// (0 to 360) and its speed in knots.
    SetWind {
        /// Direction the wind blows from, in degrees true.
        direction_deg: u16,
        /// Wind speed in knots.
        speed_kt: u16,
    },
    /// No movements at all until lifted.
    GroundStop {
        /// Why movements are stopped; lifting names the same reason.
        reason: Reason,
    },
    /// Lifts the ground stop made for `reason`.
    LiftGroundStop {
        /// The ground stop to lift.
        reason: Reason,
    },
}

/// Why a command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The flight identifier has been used before.
    DuplicateFlight,
    /// The aircraft type is not in the table in `data.rs`.
    UnknownAircraft,
    /// No runway at this airport is long enough for the aircraft type.
    NoSuitableRunway,
    /// The flight is not waiting here.
    UnknownFlight,
    /// No runway at this airport has that name.
    UnknownRunway,
    /// The wind direction is above 360 degrees.
    InvalidWind,
}

/// Why an arrival diverted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DivertReason {
    /// It held for as long as the airport allows without getting a slot.
    HoldLimit,
}

/// What the airport decided as a result of a command.
///
/// Every variant records `at_ms`, the simulated time it happened (or, for
/// `RunwayInspection`, `from_ms`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Output {
    /// The flight joined the queue.
    Accepted {
        /// The flight accepted.
        flight: String,
        /// When it was accepted.
        at_ms: Ms,
    },
    /// The command was refused and changed nothing.
    Rejected {
        /// The flight or runway the command named, if any.
        subject: String,
        /// When it was refused.
        at_ms: Ms,
        /// Why it was refused.
        reason: RejectReason,
    },
    /// The flight crossed the threshold of `runway` at `at_ms` and was clear
    /// of it at `vacated_ms`.
    Landed {
        /// The flight that landed.
        flight: String,
        /// Its ICAO type designator.
        aircraft: String,
        /// The runway direction used, e.g. `27L`.
        runway: String,
        /// When it crossed the threshold.
        at_ms: Ms,
        /// When it was clear of the runway.
        vacated_ms: Ms,
        /// How long after its requested time the flight landed.
        delay_ms: Ms,
        /// The emergency it had declared, if any.
        emergency: Option<Emergency>,
    },
    /// The flight started its take-off roll on `runway` at `at_ms` and was
    /// airborne at `vacated_ms`.
    Departed {
        /// The flight that departed.
        flight: String,
        /// Its ICAO type designator.
        aircraft: String,
        /// The runway direction used, e.g. `27L`.
        runway: String,
        /// When it started its take-off roll.
        at_ms: Ms,
        /// When it was airborne.
        vacated_ms: Ms,
        /// How long after its requested time the flight departed.
        delay_ms: Ms,
        /// The emergency it had declared, if any.
        emergency: Option<Emergency>,
    },
    /// The arrival could not be accepted and must go elsewhere.
    Diverted {
        /// The flight that diverted.
        flight: String,
        /// When it diverted.
        at_ms: Ms,
        /// Why it diverted.
        reason: DivertReason,
    },
    /// A waiting flight declared or upgraded an emergency and moves up the
    /// queue.
    EmergencyDeclared {
        /// The flight in difficulty.
        flight: String,
        /// When it declared.
        at_ms: Ms,
        /// How serious the emergency is.
        kind: Emergency,
    },
    /// `runway` is the full name, e.g. `09R/27L`.
    RunwayClosed {
        /// The full runway name.
        runway: String,
        /// When it closed.
        at_ms: Ms,
        /// Why it closed.
        reason: Reason,
    },
    /// A closure was lifted. `open` says whether the runway is now usable or
    /// is still closed for another reason.
    RunwayOpened {
        /// The full runway name.
        runway: String,
        /// When the closure was lifted.
        at_ms: Ms,
        /// The closure that was lifted.
        reason: Reason,
        /// Whether no closure remains.
        open: bool,
    },
    /// The runway is shut for inspection after a mayday landing.
    RunwayInspection {
        /// The full runway name.
        runway: String,
        /// When the inspection starts.
        from_ms: Ms,
        /// When the runway is usable again.
        until_ms: Ms,
    },
    /// The set of runway directions in use changed (wind or closures).
    ActiveRunways {
        /// When the set changed.
        at_ms: Ms,
        /// The runway directions now in use, e.g. `27L`.
        runways: Vec<String>,
    },
    /// The wind changed.
    WindChanged {
        /// When it changed.
        at_ms: Ms,
        /// Direction the wind blows from, in degrees true.
        direction_deg: u16,
        /// Wind speed in knots.
        speed_kt: u16,
    },
    /// All movements stopped.
    GroundStopStarted {
        /// When the stop began.
        at_ms: Ms,
        /// Why movements stopped.
        reason: Reason,
    },
    /// A ground stop was lifted. `clear` says whether movements may resume
    /// or another ground stop is still in force.
    GroundStopLifted {
        /// When the stop was lifted.
        at_ms: Ms,
        /// The ground stop that was lifted.
        reason: Reason,
        /// Whether no ground stop remains.
        clear: bool,
    },
}
