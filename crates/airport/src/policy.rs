//! Small generators of commands for things that follow a schedule: curfew,
//! maintenance and wind. These were the "policies" of the old calculator.

use crate::command::{Command, Reason};
use crate::scenario::Timed;
use crate::{MS_PER_DAY, Ms};

/// A nightly curfew: a ground stop from `start_of_day_ms` to `end_of_day_ms`
/// (both measured from midnight) on every day that starts before `until_ms`.
/// If the end is not after the start, the curfew runs past midnight.
#[must_use]
pub fn curfew(start_of_day_ms: Ms, end_of_day_ms: Ms, until_ms: Ms) -> Vec<Timed> {
    let length = if end_of_day_ms > start_of_day_ms {
        end_of_day_ms - start_of_day_ms
    } else {
        end_of_day_ms + MS_PER_DAY - start_of_day_ms
    };
    let reason = Reason::Curfew;
    let mut commands = Vec::new();
    let mut start = start_of_day_ms;
    while start < until_ms {
        commands.push(Timed {
            at_ms: start,
            command: Command::GroundStop { reason },
        });
        commands.push(Timed {
            at_ms: start + length,
            command: Command::LiftGroundStop { reason },
        });
        start += MS_PER_DAY;
    }
    commands
}

/// Regular maintenance: `runway` closes for `duration_ms` starting at
/// `first_ms` and then every `every_ms`, for closures starting before
/// `until_ms`. An `every_ms` of zero means once only.
#[must_use]
pub fn maintenance(
    runway: &str,
    first_ms: Ms,
    duration_ms: Ms,
    every_ms: Ms,
    until_ms: Ms,
) -> Vec<Timed> {
    let reason = Reason::Maintenance;
    let mut commands = Vec::new();
    let mut start = first_ms;
    while start < until_ms {
        commands.push(Timed {
            at_ms: start,
            command: Command::CloseRunway {
                runway: runway.to_owned(),
                reason,
            },
        });
        commands.push(Timed {
            at_ms: start + duration_ms,
            command: Command::OpenRunway {
                runway: runway.to_owned(),
                reason,
            },
        });
        if every_ms == 0 {
            break;
        }
        start += every_ms;
    }
    commands
}

/// A wind forecast: `(time, direction in degrees, speed in knots)`.
#[must_use]
pub fn wind_schedule(changes: &[(Ms, u16, u16)]) -> Vec<Timed> {
    changes
        .iter()
        .map(|&(at_ms, direction_deg, speed_kt)| Timed {
            at_ms,
            command: Command::SetWind {
                direction_deg,
                speed_kt,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MS_PER_HOUR;

    #[test]
    fn curfew_past_midnight_repeats_daily() {
        let commands = curfew(23 * MS_PER_HOUR, 6 * MS_PER_HOUR, 2 * MS_PER_DAY);
        let times: Vec<Ms> = commands.iter().map(|c| c.at_ms / MS_PER_HOUR).collect();
        assert_eq!(times, [23, 30, 47, 54]);
        assert!(matches!(commands[0].command, Command::GroundStop { .. }));
        assert!(matches!(
            commands[1].command,
            Command::LiftGroundStop { .. }
        ));
    }

    #[test]
    fn maintenance_closes_and_reopens() {
        let commands = maintenance(
            "09L",
            MS_PER_HOUR,
            MS_PER_HOUR / 2,
            MS_PER_DAY,
            2 * MS_PER_DAY,
        );
        assert_eq!(commands.len(), 4);
        assert_eq!(commands[1].at_ms, MS_PER_HOUR + MS_PER_HOUR / 2);
        assert_eq!(commands[2].at_ms, MS_PER_DAY + MS_PER_HOUR);
        assert_eq!(maintenance("09L", 0, 10, 0, 100).len(), 2);
    }
}
