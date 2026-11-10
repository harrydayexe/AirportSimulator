//! What goes into an airport (commands) and what comes out (outputs).

use serde::{Deserialize, Serialize};

use crate::Ms;

/// Why a runway is closed or movements are stopped. Closures and ground
/// stops are tracked per reason, so a storm closure and a maintenance
/// closure of the same runway can overlap and be lifted separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Maintenance,
    Storm,
    Curfew,
    Incident,
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
    AdvanceTime { now_ms: Ms },
    /// A flight wants to land, no earlier than `earliest_ms`.
    RequestLanding {
        flight: String,
        /// ICAO type designator, e.g. `A320`.
        aircraft: String,
        earliest_ms: Ms,
    },
    /// A flight wants to take off, no earlier than `earliest_ms`.
    RequestDeparture {
        flight: String,
        aircraft: String,
        earliest_ms: Ms,
    },
    /// A waiting flight has declared an emergency.
    DeclareEmergency { flight: String, kind: Emergency },
    /// A runway is unusable until opened again. `runway` is either end's
    /// name (`27L`) or the full name (`09R/27L`).
    CloseRunway { runway: String, reason: Reason },
    /// Lifts the closure made for `reason`. The runway is usable again once
    /// no closure remains.
    OpenRunway { runway: String, reason: Reason },
    /// The wind is now this: the direction it blows from in degrees true
    /// (0 to 360) and its speed in knots.
    SetWind { direction_deg: u16, speed_kt: u16 },
    /// No movements at all until lifted.
    GroundStop { reason: Reason },
    /// Lifts the ground stop made for `reason`.
    LiftGroundStop { reason: Reason },
}

/// Why a command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The flight identifier has been used before.
    DuplicateFlight,
    UnknownAircraft,
    /// No runway at this airport is long enough for the aircraft type.
    NoSuitableRunway,
    /// The flight is not waiting here.
    UnknownFlight,
    UnknownRunway,
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
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Output {
    /// The flight joined the queue.
    Accepted {
        flight: String,
        at_ms: Ms,
    },
    /// The command was refused and changed nothing.
    Rejected {
        /// The flight or runway the command named, if any.
        subject: String,
        at_ms: Ms,
        reason: RejectReason,
    },
    /// The flight crossed the threshold of `runway` at `at_ms` and was clear
    /// of it at `vacated_ms`.
    Landed {
        flight: String,
        aircraft: String,
        /// The runway direction used, e.g. `27L`.
        runway: String,
        at_ms: Ms,
        vacated_ms: Ms,
        /// How long after its requested time the flight landed.
        delay_ms: Ms,
        emergency: Option<Emergency>,
    },
    /// The flight started its take-off roll on `runway` at `at_ms` and was
    /// airborne at `vacated_ms`.
    Departed {
        flight: String,
        aircraft: String,
        runway: String,
        at_ms: Ms,
        vacated_ms: Ms,
        delay_ms: Ms,
        emergency: Option<Emergency>,
    },
    /// The arrival could not be accepted and must go elsewhere.
    Diverted {
        flight: String,
        at_ms: Ms,
        reason: DivertReason,
    },
    EmergencyDeclared {
        flight: String,
        at_ms: Ms,
        kind: Emergency,
    },
    /// `runway` is the full name, e.g. `09R/27L`.
    RunwayClosed {
        runway: String,
        at_ms: Ms,
        reason: Reason,
    },
    /// A closure was lifted. `open` says whether the runway is now usable or
    /// is still closed for another reason.
    RunwayOpened {
        runway: String,
        at_ms: Ms,
        reason: Reason,
        open: bool,
    },
    /// The runway is shut for inspection after a mayday landing.
    RunwayInspection {
        runway: String,
        from_ms: Ms,
        until_ms: Ms,
    },
    /// The set of runway directions in use changed (wind or closures).
    ActiveRunways {
        at_ms: Ms,
        runways: Vec<String>,
    },
    WindChanged {
        at_ms: Ms,
        direction_deg: u16,
        speed_kt: u16,
    },
    GroundStopStarted {
        at_ms: Ms,
        reason: Reason,
    },
    /// A ground stop was lifted. `clear` says whether movements may resume
    /// or another ground stop is still in force.
    GroundStopLifted {
        at_ms: Ms,
        reason: Reason,
        clear: bool,
    },
}
