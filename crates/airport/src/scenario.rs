//! A scenario is an airport and the full list of commands to feed it.
//! Running one is the whole of a Phase 1 simulation.

use serde::{Deserialize, Serialize};

use crate::command::{Command, Emergency, Output};
use crate::config::{AirportConfig, ConfigError};
use crate::state::{Airport, Snapshot};
use crate::traffic::{TrafficParams, generate};
use crate::{MS_PER_HOUR, Ms, policy};

/// A command and the simulated time at which it is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timed {
    /// When the command is sent.
    pub at_ms: Ms,
    /// The command to send.
    pub command: Command,
}

/// An airport and every command it will receive, in order. Time passes only
/// through the `advance_time` commands in the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    /// The airport to build.
    pub airport: AirportConfig,
    /// The commands to apply to it, in order.
    pub commands: Vec<Command>,
}

/// Everything a run produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResult {
    /// Every output, in the order the airport produced them.
    pub outputs: Vec<Output>,
    /// The state before the first command.
    pub start: Snapshot,
    /// The state after the last command.
    pub end: Snapshot,
}

/// Turns timed commands into the list an airport is fed: the commands in
/// time order (ties keep their given order), with an `advance_time` before
/// each new moment and one every `tick_ms` up to `end_ms`.
#[must_use]
pub fn script(mut timed: Vec<Timed>, tick_ms: Ms, end_ms: Ms) -> Vec<Command> {
    timed.sort_by_key(|t| t.at_ms);
    let tick_ms = tick_ms.max(1);
    let mut commands = Vec::new();
    let mut now = 0;
    let mut next_tick = tick_ms;
    let mut advance = |commands: &mut Vec<Command>, to: Ms| {
        while next_tick < to {
            commands.push(Command::AdvanceTime { now_ms: next_tick });
            next_tick += tick_ms;
        }
        if to > now {
            commands.push(Command::AdvanceTime { now_ms: to });
            now = to;
        }
        if next_tick == to {
            next_tick += tick_ms;
        }
    };
    for t in timed {
        advance(&mut commands, t.at_ms);
        commands.push(t.command);
    }
    advance(&mut commands, end_ms);
    commands
}

/// Feeds every command of `scenario` to a new airport.
///
/// # Errors
/// Returns an error if the airport configuration is invalid.
pub fn run(scenario: &Scenario) -> Result<RunResult, ConfigError> {
    let mut airport = Airport::new(scenario.airport.clone())?;
    let start = airport.snapshot();
    let mut outputs = Vec::new();
    for command in &scenario.commands {
        outputs.extend(airport.apply(command));
    }
    Ok(RunResult {
        outputs,
        start,
        end: airport.snapshot(),
    })
}

/// How often the demonstration scenario advances the clock.
pub const DEMO_TICK_MS: Ms = 5_000;

/// The demonstration: seeded traffic at the demo airport over `hours`
/// hours, interrupted by a little of everything.
///
/// - A westerly wind to begin with, so the two parallel runways are in use.
/// - Maintenance closes runway 09R/27L from 20% to 35% of the way through.
/// - From 50% to 66% a southerly gale puts the parallels out of crosswind
///   limits, leaving only the short crossing runway. Aircraft that need a
///   longer one hold, and divert if it lasts too long for them.
/// - The first arrival due after 80% declares a mayday.
///
/// The run continues for two hours after the last request so the queue can
/// drain.
#[must_use]
pub fn demo(seed: u64, hours: u32, arrivals_per_hour: u32, departures_per_hour: u32) -> Scenario {
    let duration = Ms::from(hours.max(1)) * MS_PER_HOUR;
    let percent = |p: Ms| duration / 100 * p;
    let mut timed = generate(&TrafficParams::new(
        seed,
        duration,
        arrivals_per_hour,
        departures_per_hour,
    ));

    let mayday = timed.iter().find_map(|t| match &t.command {
        Command::RequestLanding {
            flight,
            earliest_ms,
            ..
        } if *earliest_ms >= percent(80) => Some(Timed {
            at_ms: earliest_ms.saturating_sub(5 * 60_000).max(t.at_ms),
            command: Command::DeclareEmergency {
                flight: flight.clone(),
                kind: Emergency::Mayday,
            },
        }),
        _ => None,
    });

    timed.extend(policy::wind_schedule(&[
        (0, 270, 10),
        (percent(50), 180, 32),
        (percent(66), 250, 14),
    ]));
    timed.extend(policy::maintenance(
        "09R/27L",
        percent(20),
        percent(15),
        0,
        duration,
    ));
    timed.extend(mayday);

    Scenario {
        airport: AirportConfig::demo(),
        commands: script(timed, DEMO_TICK_MS, duration + 2 * MS_PER_HOUR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wind(at_ms: Ms) -> Timed {
        Timed {
            at_ms,
            command: Command::SetWind {
                direction_deg: 0,
                speed_kt: u16::try_from(at_ms).unwrap(),
            },
        }
    }

    #[test]
    fn script_interleaves_ticks_and_commands() {
        let commands = script(vec![wind(25), wind(0), wind(20), wind(25)], 10, 40);
        let summary: Vec<String> = commands
            .iter()
            .map(|c| match c {
                Command::AdvanceTime { now_ms } => format!("t{now_ms}"),
                Command::SetWind { speed_kt, .. } => format!("w{speed_kt}"),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            summary,
            ["w0", "t10", "t20", "w20", "t25", "w25", "w25", "t30", "t40"]
        );
    }

    #[test]
    fn a_scenario_survives_a_round_trip_through_json() {
        let scenario = demo(3, 1, 20, 20);
        let json = serde_json::to_string(&scenario).unwrap();
        let back: Scenario = serde_json::from_str(&json).unwrap();
        assert_eq!(back, scenario);
    }
}
