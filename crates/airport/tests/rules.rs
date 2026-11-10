//! Unit tests of the airport state machine: given this state and this
//! command, expect that state. Expected values are worked out by hand from
//! the tables in `data.rs`, and the working is shown beside each one.

use airport::{
    Airport, AirportConfig, Command, Emergency, MS_PER_HOUR, MS_PER_MINUTE, Ms, Output, Reason,
    RejectReason, RunwayConfig,
};

fn land(flight: &str, aircraft: &str, earliest_ms: Ms) -> Command {
    Command::RequestLanding {
        flight: flight.into(),
        aircraft: aircraft.into(),
        earliest_ms,
    }
}

fn depart(flight: &str, aircraft: &str, earliest_ms: Ms) -> Command {
    Command::RequestDeparture {
        flight: flight.into(),
        aircraft: aircraft.into(),
        earliest_ms,
    }
}

fn wind(direction_deg: u16, speed_kt: u16) -> Command {
    Command::SetWind {
        direction_deg,
        speed_kt,
    }
}

fn close(runway: &str, reason: Reason) -> Command {
    Command::CloseRunway {
        runway: runway.into(),
        reason,
    }
}

fn open(runway: &str, reason: Reason) -> Command {
    Command::OpenRunway {
        runway: runway.into(),
        reason,
    }
}

fn emergency(flight: &str, kind: Emergency) -> Command {
    Command::DeclareEmergency {
        flight: flight.into(),
        kind,
    }
}

fn single() -> Airport {
    Airport::new(AirportConfig::single_runway()).unwrap()
}

fn demo() -> Airport {
    Airport::new(AirportConfig::demo()).unwrap()
}

/// Applies commands, panicking if any is rejected.
fn send(airport: &mut Airport, commands: &[Command]) -> Vec<Output> {
    let mut out = Vec::new();
    for command in commands {
        out.extend(airport.apply(command));
    }
    assert!(
        !out.iter().any(|o| matches!(o, Output::Rejected { .. })),
        "unexpected rejection in {out:?}"
    );
    out
}

/// Advances the clock to `to` in steps of `step`.
fn advance(airport: &mut Airport, to: Ms, step: Ms) -> Vec<Output> {
    let mut out = Vec::new();
    let mut now = airport.now_ms();
    while now < to {
        now = (now + step).min(to);
        out.extend(airport.apply(&Command::AdvanceTime { now_ms: now }));
    }
    out
}

/// `(flight, runway, time)` of every landing and take-off, in output order.
fn movements(outputs: &[Output]) -> Vec<(&str, &str, Ms)> {
    outputs
        .iter()
        .filter_map(|o| match o {
            Output::Landed {
                flight,
                runway,
                at_ms,
                ..
            }
            | Output::Departed {
                flight,
                runway,
                at_ms,
                ..
            } => Some((flight.as_str(), runway.as_str(), *at_ms)),
            _ => None,
        })
        .collect()
}

fn times(outputs: &[Output]) -> Vec<Ms> {
    movements(outputs).iter().map(|m| m.2).collect()
}

fn order(outputs: &[Output]) -> Vec<&str> {
    movements(outputs).iter().map(|m| m.0).collect()
}

// ---------------------------------------------------------------------------
// Done-when: a flooded single runway matches the hand-worked rate.
// ---------------------------------------------------------------------------

/// Floods the single runway with `count` identical flights, all wanting to
/// move at time zero, and returns the movement times over three hours.
fn flood(mut airport: Airport, landing: bool, aircraft: &str, count: u32, step: Ms) -> Vec<Ms> {
    for i in 0..count {
        let id = format!("F{i}");
        let command = if landing {
            land(&id, aircraft, 0)
        } else {
            depart(&id, aircraft, 0)
        };
        send(&mut airport, &[command]);
    }
    times(&advance(&mut airport, 3 * MS_PER_HOUR, step))
}

fn in_second_hour(times: &[Ms]) -> usize {
    times
        .iter()
        .filter(|t| (MS_PER_HOUR..2 * MS_PER_HOUR).contains(*t))
        .count()
}

#[test]
fn flooded_runway_lands_a320s_at_the_hand_worked_rate() {
    // A320 behind A320: both medium, so the 3 nm radar minimum applies.
    // At 137 kt that is 3 / 137 h = 78 832.1 ms, rounded up to 78 833 ms.
    // The A320 clears the runway in 50 s, which is less, so separation is
    // the limit. Landings fall at k * 78 833 ms. In the second hour those
    // are k = 46 (3 626 318 ms) to k = 91 (7 173 803 ms): 46 landings,
    // matching 3 600 / 78.833 = 45.7 an hour.
    let times = flood(single(), true, "A320", 300, 1_000);
    for (k, t) in times.iter().enumerate() {
        assert_eq!(*t, k as Ms * 78_833);
    }
    assert_eq!(in_second_hour(&times), 46);
    // Three hours hold k = 0 to 136 (136 * 78 833 = 10 721 288 ms).
    assert_eq!(times.len(), 137);
}

#[test]
fn the_rate_does_not_depend_on_how_often_the_clock_ticks() {
    let fine = flood(single(), true, "A320", 300, 1_000);
    let coarse = flood(single(), true, "A320", 300, 7 * MS_PER_MINUTE);
    assert_eq!(fine, coarse);
}

#[test]
fn flooded_runway_lands_heavies_at_the_hand_worked_rate() {
    // B77W behind B77W: heavy behind heavy is 4 nm. At 149 kt that is
    // 4 / 149 h = 96 644.3 ms, rounded up to 96 645 ms: 37.2 an hour.
    // Second hour: k = 38 (3 672 510 ms) to k = 74 (7 151 730 ms), 37 landings.
    let times = flood(single(), true, "B77W", 200, 1_000);
    assert!(times.windows(2).all(|w| w[1] - w[0] == 96_645));
    assert_eq!(in_second_hour(&times), 37);
}

#[test]
fn flooded_runway_departs_a320s_at_the_hand_worked_rate() {
    // Medium behind medium has no wake minimum, so the one-minute departure
    // interval applies. The take-off roll takes 40 s, which is less.
    // One a minute is exactly 60 an hour.
    let times = flood(single(), false, "A320", 300, 1_000);
    assert!(times.windows(2).all(|w| w[1] - w[0] == 60_000));
    assert_eq!(in_second_hour(&times), 60);
}

#[test]
fn slow_exits_make_runway_occupancy_the_limit() {
    // With exits that cost an extra 40 s, an A320 occupies the runway for
    // 50 + 40 = 90 s, more than the 78.8 s separation. One landing every
    // 90 s is exactly 40 an hour.
    let mut config = AirportConfig::single_runway();
    config.runways[0].exit_extra_ms = 40_000;
    let times = flood(Airport::new(config).unwrap(), true, "A320", 300, 1_000);
    assert!(times.windows(2).all(|w| w[1] - w[0] == 90_000));
    assert_eq!(in_second_hour(&times), 40);
}

// ---------------------------------------------------------------------------
// Aspect 1: landing separation.
// ---------------------------------------------------------------------------

#[test]
fn a_light_stays_six_miles_behind_a_heavy() {
    // PC12 (85 kt) behind B77W (149 kt) on a 6 nm final. The slower
    // follower is closest when the heavy starts its approach, so:
    // (6 + 6) / 85 h - 6 / 149 h = 508 235.3 - 144 966.4 = 363 268.9 ms.
    let mut a = single();
    send(&mut a, &[land("H", "B77W", 0), land("L", "PC12", 0)]);
    assert_eq!(times(&advance(&mut a, MS_PER_HOUR, 1_000)), [0, 363_269]);
}

#[test]
fn a_heavy_behind_a_light_needs_only_the_radar_minimum() {
    // B77W behind PC12: no wake minimum, 3 nm at 149 kt = 72 483.2 ms.
    let mut a = single();
    send(&mut a, &[land("L", "PC12", 0), land("H", "B77W", 0)]);
    assert_eq!(times(&advance(&mut a, MS_PER_HOUR, 1_000)), [0, 72_484]);
}

#[test]
fn a_super_needs_the_widest_gaps_behind_it() {
    // A320 (137 kt) behind A388 (138 kt): 7 nm, slower follower.
    // (6 + 7) / 137 h - 6 / 138 h = 341 605.8 - 156 521.7 = 185 084.1 ms.
    let mut a = single();
    send(&mut a, &[land("S", "A388", 0), land("M", "A320", 0)]);
    assert_eq!(times(&advance(&mut a, MS_PER_HOUR, 1_000)), [0, 185_085]);
}

#[test]
fn departures_wait_two_minutes_behind_a_heavy() {
    let mut a = single();
    send(
        &mut a,
        &[
            depart("H", "B77W", 0),
            depart("M", "A320", 0),
            depart("M2", "A320", 0),
            depart("L", "PC12", 0),
        ],
    );
    // Heavy, then medium 2 min later, then medium 1 min later (no wake
    // minimum), then light 2 min behind the medium.
    assert_eq!(
        times(&advance(&mut a, MS_PER_HOUR, 1_000)),
        [0, 120_000, 180_000, 300_000]
    );
}

#[test]
fn wake_still_applies_across_a_departure_in_between() {
    // Heavy lands at 0 and is clear at 55 s. A departure rolls at 55 s and
    // is airborne 40 s later, at 95 s. The light arrival still owes the
    // heavy its wake gap (363 269 ms, as above), not just the 95 s.
    let mut a = single();
    send(
        &mut a,
        &[
            land("H", "B77W", 0),
            depart("D", "A320", 0),
            land("L", "PC12", 0),
        ],
    );
    assert_eq!(
        times(&advance(&mut a, MS_PER_HOUR, 1_000)),
        [0, 55_000, 363_269]
    );
}

// ---------------------------------------------------------------------------
// Aspect 2: flight performance.
// ---------------------------------------------------------------------------

#[test]
fn the_order_of_the_queue_changes_how_many_get_through() {
    // Two B738 (144 kt) and two AT76 (113 kt), all medium, 3 nm apart.
    //   fast behind fast: 3 / 144 h                 =  75 000 ms
    //   slow behind slow: 3 / 113 h                 =  95 575.2 -> 95 576 ms
    //   fast behind slow: 3 / 144 h                 =  75 000 ms
    //   slow behind fast: 9 / 113 h - 6 / 144 h     = 136 725.7 -> 136 726 ms
    let run = |types: [&str; 4]| {
        let mut a = single();
        for (i, ty) in types.iter().enumerate() {
            // One millisecond apart to fix the order.
            send(&mut a, &[land(&format!("F{i}"), ty, i as Ms)]);
        }
        times(&advance(&mut a, MS_PER_HOUR, 1_000))
    };
    // Grouped: 75 000 + 136 726 + 95 576.
    assert_eq!(
        run(["B738", "B738", "AT76", "AT76"]),
        [0, 75_000, 211_726, 307_302]
    );
    // Alternating: 136 726 + 75 000 + 136 726. The same four aircraft take
    // 41 s longer.
    assert_eq!(
        run(["B738", "AT76", "B738", "AT76"]),
        [0, 136_726, 211_726, 348_452]
    );
}

#[test]
fn an_aircraft_is_refused_where_no_runway_is_long_enough() {
    // An A380 needs 3 000 m.
    let config = AirportConfig::new("Short", vec![RunwayConfig::new("09", "27", 90, 2_400)]);
    let mut a = Airport::new(config).unwrap();
    assert_eq!(
        a.apply(&land("BIG", "A388", 0)),
        [Output::Rejected {
            subject: "BIG".into(),
            at_ms: 0,
            reason: RejectReason::NoSuitableRunway
        }]
    );
    assert!(matches!(
        a.apply(&land("SMALL", "E190", 0))[..],
        [Output::Accepted { .. }]
    ));
}

#[test]
fn an_aircraft_waits_for_a_runway_long_enough() {
    // The demo airport's parallels are closed, leaving the 2 400 m runway.
    // The B738 (2 300 m) can use it; the A380 has to wait for a parallel.
    let mut a = demo();
    send(
        &mut a,
        &[
            close("09L", Reason::Storm),
            close("09R", Reason::Storm),
            land("BIG", "A388", 0),
            land("SMALL", "B738", 0),
        ],
    );
    let out = advance(&mut a, 10 * MS_PER_MINUTE, 1_000);
    assert_eq!(movements(&out), [("SMALL", "18", 0)]);
    send(&mut a, &[open("09L", Reason::Storm)]);
    let out = advance(&mut a, 20 * MS_PER_MINUTE, 1_000);
    assert_eq!(movements(&out), [("BIG", "09L", 10 * MS_PER_MINUTE)]);
}

// ---------------------------------------------------------------------------
// Aspect 3: runway exit time.
// ---------------------------------------------------------------------------

#[test]
fn a_departure_waits_for_the_arrival_to_turn_off() {
    // A medium arrival occupies the runway for 50 s.
    let mut a = single();
    send(&mut a, &[land("A", "A320", 0), depart("D", "A320", 0)]);
    let out = advance(&mut a, MS_PER_HOUR, 1_000);
    assert_eq!(times(&out), [0, 50_000]);
    assert!(matches!(
        out[0],
        Output::Landed {
            vacated_ms: 50_000,
            ..
        }
    ));
}

#[test]
fn an_arrival_waits_for_the_departure_to_lift_off() {
    // A heavy's take-off roll occupies the runway for 50 s.
    let mut a = single();
    send(&mut a, &[depart("D", "B77W", 0), land("A", "A320", 0)]);
    assert_eq!(times(&advance(&mut a, MS_PER_HOUR, 1_000)), [0, 50_000]);
}

#[test]
fn occupancy_depends_on_the_aircraft_and_the_exits() {
    // Light 40 s, medium 50 s, heavy 55 s, super 60 s; plus 5 s on the demo
    // airport's southern runway.
    for (aircraft, expected) in [
        ("PC12", 40_000),
        ("A320", 50_000),
        ("B77W", 55_000),
        ("A388", 60_000),
    ] {
        let mut a = single();
        send(&mut a, &[land("A", aircraft, 0), depart("D", "PC12", 0)]);
        assert_eq!(times(&advance(&mut a, MS_PER_HOUR, 1_000)), [0, expected]);
    }
    let mut a = demo();
    send(
        &mut a,
        &[
            close("09L", Reason::Maintenance),
            land("A", "A320", 0),
            depart("D", "A320", 0),
        ],
    );
    assert_eq!(times(&advance(&mut a, MS_PER_HOUR, 1_000)), [0, 55_000]);
}

#[test]
fn a_runway_holds_one_aircraft_at_a_time() {
    let mut a = single();
    for i in 0..40 {
        let ty = ["A320", "B77W", "PC12", "A388"][i % 4];
        let cmd = if i % 3 == 0 {
            depart(&format!("F{i}"), ty, 0)
        } else {
            land(&format!("F{i}"), ty, 0)
        };
        send(&mut a, &[cmd]);
    }
    let out = advance(&mut a, 3 * MS_PER_HOUR, 1_000);
    let spans: Vec<(Ms, Ms)> = out
        .iter()
        .filter_map(|o| match o {
            Output::Landed {
                at_ms, vacated_ms, ..
            }
            | Output::Departed {
                at_ms, vacated_ms, ..
            } => Some((*at_ms, *vacated_ms)),
            _ => None,
        })
        .collect();
    assert_eq!(spans.len(), 40);
    assert!(spans.windows(2).all(|w| w[1].0 >= w[0].1));
}

// ---------------------------------------------------------------------------
// Aspect 4: emergencies.
// ---------------------------------------------------------------------------

#[test]
fn an_emergency_jumps_the_queue() {
    let mut a = single();
    for i in 0..5 {
        send(&mut a, &[land(&format!("F{i}"), "A320", 0)]);
    }
    let out = send(&mut a, &[emergency("F4", Emergency::PanPan)]);
    assert_eq!(
        out,
        [Output::EmergencyDeclared {
            flight: "F4".into(),
            at_ms: 0,
            kind: Emergency::PanPan
        }]
    );
    let out = advance(&mut a, MS_PER_HOUR, 1_000);
    assert_eq!(order(&out), ["F4", "F0", "F1", "F2", "F3"]);
    // A pan-pan does not close the runway: gaps stay at 78 833 ms.
    assert!(times(&out).windows(2).all(|w| w[1] - w[0] == 78_833));
}

#[test]
fn a_mayday_goes_ahead_of_a_pan_pan() {
    let mut a = single();
    for i in 0..4 {
        send(&mut a, &[land(&format!("F{i}"), "A320", 0)]);
    }
    send(
        &mut a,
        &[
            emergency("F2", Emergency::PanPan),
            emergency("F3", Emergency::Mayday),
        ],
    );
    assert_eq!(
        order(&advance(&mut a, MS_PER_HOUR, 1_000)),
        ["F3", "F2", "F0", "F1"]
    );
}

#[test]
fn a_pan_pan_can_be_upgraded_but_a_mayday_cannot_be_downgraded() {
    let mut a = single();
    send(&mut a, &[land("F", "A320", 0)]);
    assert_eq!(send(&mut a, &[emergency("F", Emergency::PanPan)]).len(), 1);
    assert_eq!(send(&mut a, &[emergency("F", Emergency::Mayday)]).len(), 1);
    assert_eq!(send(&mut a, &[emergency("F", Emergency::PanPan)]).len(), 0);
    let out = advance(&mut a, MS_PER_MINUTE, 1_000);
    assert!(matches!(
        out[0],
        Output::Landed {
            emergency: Some(Emergency::Mayday),
            ..
        }
    ));
}

#[test]
fn the_runway_is_inspected_after_a_mayday_landing() {
    let mut a = single();
    send(
        &mut a,
        &[
            land("NEXT", "A320", 0),
            depart("DEP", "A320", 0),
            land("MAYDAY", "A320", 0),
            emergency("MAYDAY", Emergency::Mayday),
        ],
    );
    let out = advance(&mut a, MS_PER_HOUR, 1_000);
    // The mayday lands at 0 and turns off at 50 s. The runway is then shut
    // for the 15 minute inspection, until 950 s. The arrival lands then,
    // and the departure follows once it has turned off 50 s later.
    assert_eq!(
        movements(&out),
        [
            ("MAYDAY", "09", 0),
            ("NEXT", "09", 950_000),
            ("DEP", "09", 1_000_000)
        ]
    );
    assert!(out.contains(&Output::RunwayInspection {
        runway: "09/27".into(),
        from_ms: 50_000,
        until_ms: 950_000
    }));
}

#[test]
fn others_keep_moving_until_they_would_delay_the_emergency() {
    // A mayday is ten minutes out. Flights in the queue keep landing every
    // 78 833 ms while each can still be followed by the mayday on time:
    // the last one must land by 600 000 - 78 833 = 521 167 ms. Landings at
    // k * 78 833 fit up to k = 6 (472 998 ms). The mayday then lands at
    // exactly ten minutes, and the rest wait out the inspection.
    let mut a = single();
    for i in 0..10 {
        send(&mut a, &[land(&format!("F{i}"), "A320", 0)]);
    }
    send(
        &mut a,
        &[
            land("MAYDAY", "A320", 10 * MS_PER_MINUTE),
            emergency("MAYDAY", Emergency::Mayday),
        ],
    );
    let out = advance(&mut a, MS_PER_HOUR, 1_000);
    let m = movements(&out);
    assert_eq!(m[6], ("F6", "09", 472_998));
    assert_eq!(m[7], ("MAYDAY", "09", 600_000));
    assert_eq!(m[8], ("F7", "09", 600_000 + 50_000 + 15 * MS_PER_MINUTE));
}

#[test]
fn an_emergency_never_diverts() {
    let mut a = demo();
    send(
        &mut a,
        &[
            Command::GroundStop {
                reason: Reason::Storm,
            },
            land("NORMAL", "A320", 0),
            land("PAN", "A320", 0),
            emergency("PAN", Emergency::PanPan),
        ],
    );
    let out = advance(&mut a, 2 * MS_PER_HOUR, MS_PER_MINUTE);
    assert_eq!(
        out,
        [Output::Diverted {
            flight: "NORMAL".into(),
            at_ms: 45 * MS_PER_MINUTE,
            reason: airport::command::DivertReason::HoldLimit
        }]
    );
    assert_eq!(a.queue_len(), 1);
}

// ---------------------------------------------------------------------------
// Runways, wind and closures.
// ---------------------------------------------------------------------------

#[test]
fn runways_are_used_into_the_wind() {
    let mut a = demo();
    assert_eq!(a.active_runways(), ["09L", "09R"]);
    let out = send(&mut a, &[wind(270, 10)]);
    assert_eq!(a.active_runways(), ["27R", "27L"]);
    assert_eq!(
        out[1],
        Output::ActiveRunways {
            at_ms: 0,
            runways: vec!["27R".into(), "27L".into()]
        }
    );
    send(&mut a, &[wind(80, 5)]);
    assert_eq!(a.active_runways(), ["09L", "09R"]);
    // Straight across: no reason to turn the airport round.
    send(&mut a, &[wind(360, 5)]);
    assert_eq!(a.active_runways(), ["09L", "09R"]);
}

#[test]
fn a_crosswind_over_the_limit_takes_a_runway_out_of_use() {
    let mut a = demo();
    // Southerly at exactly the 25 kt limit: the parallels stay in use.
    send(&mut a, &[wind(180, 25)]);
    assert_eq!(a.active_runways(), ["09L", "09R"]);
    // One knot more and only the north-south runway is left.
    send(&mut a, &[wind(180, 26)]);
    assert_eq!(a.active_runways(), ["18"]);
    send(&mut a, &[wind(350, 40)]);
    assert_eq!(a.active_runways(), ["36"]);
    // 40 kt from 230: 40 degrees off the parallels (crosswind 25.7 kt) and
    // 50 degrees off runway 18 (30.6 kt). Nothing is usable.
    send(&mut a, &[wind(230, 40)]);
    assert!(a.active_runways().is_empty());
    // 36 kt from 230 is 23.1 kt across the parallels.
    send(&mut a, &[wind(230, 36)]);
    assert_eq!(a.active_runways(), ["27R", "27L"]);
}

#[test]
fn crossing_runways_are_never_used_together() {
    let mut a = demo();
    // Two parallels beat one crossing runway.
    assert_eq!(a.active_runways(), ["09L", "09R"]);
    // With one parallel shut it is one against one. The longer runway is
    // kept, even when the wind favours the other.
    send(&mut a, &[close("27R", Reason::Maintenance)]);
    assert_eq!(a.active_runways(), ["09R"]);
    send(&mut a, &[wind(180, 10)]);
    assert_eq!(a.active_runways(), ["09R"]);
    send(&mut a, &[close("27L", Reason::Maintenance)]);
    assert_eq!(a.active_runways(), ["18"]);
    send(&mut a, &[open("27R", Reason::Maintenance)]);
    assert_eq!(a.active_runways(), ["09L"]);
}

#[test]
fn the_longest_usable_runway_is_always_in_use() {
    // Two short parallels and a long runway crossing one of them. Using
    // both parallels would shut out every aircraft that needs the long one.
    let mut config = AirportConfig::new(
        "Cross",
        vec![
            RunwayConfig::new("09L", "27R", 90, 2_000),
            RunwayConfig::new("09R", "27L", 90, 2_000),
            RunwayConfig::new("18", "36", 180, 3_500),
        ],
    );
    config.crossings = vec![("18".into(), "09R".into())];
    let mut a = Airport::new(config).unwrap();
    assert_eq!(a.active_runways(), ["09L", "18"]);
    // Of two equal sets, the one with less crosswind wins.
    let mut config = AirportConfig::new(
        "Tie",
        vec![
            RunwayConfig::new("09", "27", 90, 3_000),
            RunwayConfig::new("18", "36", 180, 3_000),
        ],
    );
    config.crossings = vec![("18".into(), "09".into())];
    let mut a2 = Airport::new(config).unwrap();
    assert_eq!(a2.active_runways(), ["09"]);
    send(&mut a2, &[wind(170, 10)]);
    assert_eq!(a2.active_runways(), ["18"]);
    send(&mut a, &[wind(190, 10)]);
    assert_eq!(a.active_runways(), ["27R", "18"]);
}

#[test]
fn a_new_runway_waits_for_the_crossing_one_to_clear() {
    // An A320 lands on 09L at 0 and is on the runway until 50 s. At 10 s a
    // gale leaves only the crossing runway. The next arrival may not land
    // on it until the first has turned off.
    let mut a = demo();
    send(&mut a, &[land("A", "A320", 0), land("B", "A320", 0)]);
    send(&mut a, &[close("09R", Reason::Maintenance)]);
    let out = advance(&mut a, 10_000, 1_000);
    assert_eq!(movements(&out), [("A", "09L", 0)]);
    send(&mut a, &[wind(180, 30)]);
    let out = advance(&mut a, MS_PER_HOUR, 1_000);
    assert_eq!(movements(&out), [("B", "18", 50_000)]);
}

#[test]
fn nothing_moves_on_a_closed_runway() {
    let mut a = single();
    send(&mut a, &[close("27", Reason::Storm)]);
    assert!(a.active_runways().is_empty());
    send(&mut a, &[land("A", "A320", 0), depart("D", "A320", 0)]);
    assert_eq!(advance(&mut a, 20 * MS_PER_MINUTE, 1_000), []);
    let out = send(&mut a, &[open("09/27", Reason::Storm)]);
    assert_eq!(
        out[0],
        Output::RunwayOpened {
            runway: "09/27".into(),
            at_ms: 20 * MS_PER_MINUTE,
            reason: Reason::Storm,
            open: true
        }
    );
    let out = advance(&mut a, MS_PER_HOUR, 1_000);
    assert_eq!(times(&out), [1_200_000, 1_250_000]);
}

#[test]
fn overlapping_closures_are_lifted_separately() {
    let mut a = single();
    send(
        &mut a,
        &[
            close("09", Reason::Storm),
            close("09", Reason::Maintenance),
            land("A", "A320", 0),
        ],
    );
    // Closing twice for the same reason says nothing new.
    assert_eq!(a.apply(&close("09", Reason::Storm)), []);
    advance(&mut a, MS_PER_MINUTE, 1_000);
    let out = send(&mut a, &[open("09", Reason::Storm)]);
    assert!(matches!(
        out[..],
        [Output::RunwayOpened { open: false, .. }]
    ));
    assert_eq!(advance(&mut a, 2 * MS_PER_MINUTE, 1_000), []);
    send(&mut a, &[open("09", Reason::Maintenance)]);
    assert_eq!(times(&advance(&mut a, 3 * MS_PER_MINUTE, 1_000)), [120_000]);
}

#[test]
fn nothing_moves_during_a_ground_stop() {
    let mut a = demo();
    send(&mut a, &[land("A", "A320", 0), land("B", "A320", 0)]);
    let stop = |reason| Command::GroundStop { reason };
    let lift = |reason| Command::LiftGroundStop { reason };
    send(&mut a, &[stop(Reason::Storm), stop(Reason::Curfew)]);
    assert_eq!(advance(&mut a, 5 * MS_PER_MINUTE, 1_000), []);
    let out = send(&mut a, &[lift(Reason::Storm)]);
    assert!(matches!(
        out[..],
        [Output::GroundStopLifted { clear: false, .. }]
    ));
    assert_eq!(advance(&mut a, 10 * MS_PER_MINUTE, 1_000), []);
    send(&mut a, &[lift(Reason::Curfew)]);
    let out = advance(&mut a, 11 * MS_PER_MINUTE, 1_000);
    assert_eq!(
        movements(&out),
        [("A", "09L", 600_000), ("B", "09R", 600_000)]
    );
}

#[test]
fn parallel_runways_share_the_queue() {
    // Four A320s wanting to land at once on two runways: two pairs, 78 833 ms
    // apart.
    let mut a = demo();
    for i in 0..4 {
        send(&mut a, &[land(&format!("F{i}"), "A320", 0)]);
    }
    assert_eq!(
        movements(&advance(&mut a, MS_PER_HOUR, 1_000)),
        [
            ("F0", "09L", 0),
            ("F1", "09R", 0),
            ("F2", "09L", 78_833),
            ("F3", "09R", 78_833)
        ]
    );
}

// ---------------------------------------------------------------------------
// The queue.
// ---------------------------------------------------------------------------

#[test]
fn flights_are_served_in_the_order_of_the_time_they_asked_for() {
    let mut a = single();
    send(
        &mut a,
        &[
            land("LATE", "A320", 300_000),
            land("EARLY", "A320", 100_000),
            depart("MIDDLE", "A320", 200_000),
        ],
    );
    assert_eq!(
        movements(&advance(&mut a, MS_PER_HOUR, 1_000)),
        [
            ("EARLY", "09", 100_000),
            ("MIDDLE", "09", 200_000),
            ("LATE", "09", 300_000)
        ]
    );
}

#[test]
fn a_flight_cannot_move_before_it_asked() {
    let mut a = single();
    advance(&mut a, MS_PER_MINUTE, 1_000);
    // Asking at one minute for a slot at time zero gets one minute.
    send(&mut a, &[land("A", "A320", 0)]);
    let out = advance(&mut a, 2 * MS_PER_MINUTE, 1_000);
    assert!(matches!(
        out[..],
        [Output::Landed {
            at_ms: 60_000,
            delay_ms: 0,
            ..
        }]
    ));
}

#[test]
fn a_departure_slips_into_a_gap_without_delaying_anyone() {
    // Two A320 arrivals want 0 s and 200 s. A departure filed after them
    // wants 60 s. The runway is free from 50 s; the departure needs 40 s
    // and so fits well before the second arrival.
    let mut a = single();
    send(
        &mut a,
        &[
            land("A1", "A320", 0),
            land("A2", "A320", 200_000),
            depart("D", "A320", 60_000),
        ],
    );
    assert_eq!(
        movements(&advance(&mut a, MS_PER_HOUR, 1_000)),
        [("A1", "09", 0), ("D", "09", 60_000), ("A2", "09", 200_000)]
    );
}

#[test]
fn an_arrival_diverts_when_it_has_held_too_long() {
    let mut a = demo();
    send(
        &mut a,
        &[close("09L", Reason::Storm), close("09R", Reason::Storm)],
    );
    // The A380 cannot use the short runway and the long ones stay shut.
    send(&mut a, &[land("BIG", "A388", 5 * MS_PER_MINUTE)]);
    send(&mut a, &[depart("DEP", "A388", 5 * MS_PER_MINUTE)]);
    let out = advance(&mut a, 3 * MS_PER_HOUR, 30_000);
    assert_eq!(
        out,
        [Output::Diverted {
            flight: "BIG".into(),
            at_ms: 50 * MS_PER_MINUTE,
            reason: airport::command::DivertReason::HoldLimit
        }]
    );
    // Departures wait on the ground for as long as it takes.
    assert_eq!(a.queue_len(), 1);
    assert_eq!(a.stats().diverted, 1);
}

#[test]
fn bad_commands_are_rejected_and_change_nothing() {
    let mut a = single();
    send(&mut a, &[land("A", "A320", 0)]);
    let before = a.fingerprint();
    let rejected = |a: &mut Airport, command: Command| match &a.apply(&command)[..] {
        [Output::Rejected { reason, .. }] => *reason,
        other => panic!("expected a rejection, got {other:?}"),
    };
    assert_eq!(
        rejected(&mut a, land("A", "B738", 0)),
        RejectReason::DuplicateFlight
    );
    assert_eq!(
        rejected(&mut a, depart("B", "ZZZZ", 0)),
        RejectReason::UnknownAircraft
    );
    assert_eq!(
        rejected(&mut a, close("04", Reason::Storm)),
        RejectReason::UnknownRunway
    );
    assert_eq!(
        rejected(&mut a, emergency("NOBODY", Emergency::Mayday)),
        RejectReason::UnknownFlight
    );
    assert_eq!(rejected(&mut a, wind(361, 10)), RejectReason::InvalidWind);
    assert_eq!(a.stats().rejected, 5);
    assert_eq!(a.queue_len(), 1);
    // Only the count of rejections has changed.
    assert_ne!(a.fingerprint(), before);
    // A flight that has landed cannot declare an emergency or come back.
    advance(&mut a, MS_PER_MINUTE, 1_000);
    assert_eq!(
        rejected(&mut a, emergency("A", Emergency::Mayday)),
        RejectReason::UnknownFlight
    );
    assert_eq!(
        rejected(&mut a, land("A", "A320", 0)),
        RejectReason::DuplicateFlight
    );
}

#[test]
fn time_never_goes_backwards() {
    let mut a = single();
    advance(&mut a, MS_PER_MINUTE, 1_000);
    let before = a.fingerprint();
    assert_eq!(a.apply(&Command::AdvanceTime { now_ms: 5 }), []);
    assert_eq!(a.now_ms(), MS_PER_MINUTE);
    assert_eq!(a.fingerprint(), before);
}

// ---------------------------------------------------------------------------
// Done-when: the same commands give the same fingerprint.
// ---------------------------------------------------------------------------

#[test]
fn the_same_commands_give_the_same_fingerprint() {
    let scenario = airport::scenario::demo(42, 6, 40, 40);
    let first = airport::scenario::run(&scenario).unwrap();
    let second = airport::scenario::run(&scenario).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.end.fingerprint, second.end.fingerprint);

    // Fingerprints agree after every single command, not just at the end.
    let mut a = Airport::new(scenario.airport.clone()).unwrap();
    let mut b = Airport::new(scenario.airport.clone()).unwrap();
    for command in &scenario.commands {
        assert_eq!(a.apply(command), b.apply(command));
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    let other = airport::scenario::run(&airport::scenario::demo(43, 6, 40, 40)).unwrap();
    assert_ne!(first.end.fingerprint, other.end.fingerprint);
}

#[test]
fn the_fingerprint_covers_the_whole_state() {
    let base = || {
        let mut a = demo();
        send(&mut a, &[land("A", "A320", MS_PER_HOUR)]);
        a
    };
    let reference = base().fingerprint();
    assert_eq!(base().fingerprint(), reference);
    for command in [
        Command::AdvanceTime { now_ms: 1 },
        land("B", "A320", MS_PER_HOUR),
        emergency("A", Emergency::PanPan),
        close("18", Reason::Storm),
        wind(90, 1),
        Command::GroundStop {
            reason: Reason::Storm,
        },
    ] {
        let mut a = base();
        a.apply(&command);
        assert_ne!(a.fingerprint(), reference, "{command:?} left no trace");
    }
}

#[test]
fn the_demo_shows_a_little_of_everything() {
    let result = airport::scenario::run(&airport::scenario::demo(1, 6, 40, 40)).unwrap();
    let count = |f: fn(&Output) -> bool| result.outputs.iter().filter(|o| f(o)).count();
    assert!(count(|o| matches!(o, Output::Landed { .. })) > 150);
    assert!(count(|o| matches!(o, Output::Departed { .. })) > 150);
    assert_eq!(count(|o| matches!(o, Output::RunwayClosed { .. })), 1);
    assert_eq!(count(|o| matches!(o, Output::RunwayInspection { .. })), 1);
    assert!(count(|o| matches!(o, Output::ActiveRunways { .. })) >= 4);
    assert!(result.end.queue.is_empty(), "the queue drains by the end");
    let stats = result.end.stats;
    assert_eq!(stats.accepted, 480);
    assert_eq!(
        stats.accepted,
        stats.landed + stats.departed + stats.diverted
    );
}
