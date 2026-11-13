//! `airport-sim`: command-line wrapper around the host.

use std::fmt::Write as _;
use std::io::Read as _;
use std::process::ExitCode;

use host::DemoOptions;
use host::airport::Output;
use host::airport::scenario::RunResult;

const USAGE: &str = "\
usage: airport-sim <command> [options]

commands:
  demo            run the demonstration scenario and print what happened
  gen             print the demonstration scenario as JSON
  run <file>      run a scenario from a JSON file (`-` for standard input)

options for demo and gen:
  --seed N        random seed for the traffic (default 1)
  --hours N       hours of traffic (default 6)
  --arrivals N    arrivals an hour (default 40)
  --departures N  departures an hour (default 40)

options for demo and run:
  --json          print the full result as JSON
  --quiet         print only the summary
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Default)]
struct Options {
    demo: DemoOptions,
    json: bool,
    quiet: bool,
    file: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut number = |name: &str| -> Result<u64, String> {
            args.next()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| format!("{name} needs a whole number\n\n{USAGE}"))
        };
        let small = |n: u64| u32::try_from(n).map_err(|_| format!("{n} is too large"));
        match arg.as_str() {
            "--seed" => options.demo.seed = number("--seed")?,
            "--hours" => options.demo.hours = small(number("--hours")?)?,
            "--arrivals" => options.demo.arrivals_per_hour = small(number("--arrivals")?)?,
            "--departures" => options.demo.departures_per_hour = small(number("--departures")?)?,
            "--json" => options.json = true,
            "--quiet" => options.quiet = true,
            file if options.file.is_none() && (file == "-" || !file.starts_with('-')) => {
                options.file = Some(file.to_owned());
            }
            other => return Err(format!("unexpected argument {other}\n\n{USAGE}")),
        }
    }
    Ok(options)
}

fn run(args: &[String]) -> Result<String, String> {
    let Some((command, rest)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    let options = parse(rest)?;
    let scenario = match command.as_str() {
        "gen" => return Ok(host::demo_scenario_json(options.demo) + "\n"),
        "demo" => host::demo_scenario_json(options.demo),
        "run" => read(options.file.as_deref())?,
        _ => return Err(USAGE.to_owned()),
    };
    if options.json {
        return Ok(host::run_scenario_json(&scenario)? + "\n");
    }
    Ok(report(&host::run_scenario(&scenario)?, options.quiet))
}

fn read(file: Option<&str>) -> Result<String, String> {
    match file {
        None => Err(format!("run needs a scenario file\n\n{USAGE}")),
        Some("-") => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("cannot read standard input: {e}"))?;
            Ok(text)
        }
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}")),
    }
}

/// `hh:mm:ss` for a simulated time.
fn clock(ms: u64) -> String {
    let s = ms / 1_000;
    format!("{:02}:{:02}:{:02}", s / 3_600, s / 60 % 60, s % 60)
}

fn report(result: &RunResult, quiet: bool) -> String {
    let mut text = String::new();
    if !quiet {
        for output in &result.outputs {
            if let Some(line) = describe(output) {
                let _ = writeln!(text, "{line}");
            }
        }
        let _ = writeln!(text);
    }
    let end = &result.end;
    let stats = end.stats;
    let mean = |total: u64, count: u64| total.checked_div(count).unwrap_or(0) / 1_000;
    let _ = writeln!(text, "ended at     {}", clock(end.now_ms));
    let _ = writeln!(text, "accepted     {}", stats.accepted);
    let _ = writeln!(
        text,
        "landed       {} (mean delay {} s)",
        stats.landed,
        mean(stats.arrival_delay_ms, stats.landed)
    );
    let _ = writeln!(
        text,
        "departed     {} (mean delay {} s)",
        stats.departed,
        mean(stats.departure_delay_ms, stats.departed)
    );
    let _ = writeln!(text, "diverted     {}", stats.diverted);
    let _ = writeln!(text, "rejected     {}", stats.rejected);
    let _ = writeln!(text, "still queued {}", end.queue.len());
    let _ = writeln!(text, "fingerprint  {}", end.fingerprint);
    text
}

fn describe(output: &Output) -> Option<String> {
    let tag = |emergency: &Option<_>| match emergency {
        Some(kind) => format!(" [{kind:?}]").to_lowercase(),
        None => String::new(),
    };
    Some(match output {
        Output::Accepted { .. } => return None,
        Output::Rejected {
            subject,
            at_ms,
            reason,
        } => format!("{} {subject} rejected: {reason:?}", clock(*at_ms)),
        Output::Landed {
            flight,
            aircraft,
            runway,
            at_ms,
            delay_ms,
            emergency,
            ..
        } => format!(
            "{} {flight} ({aircraft}) landed on {runway}, delayed {} s{}",
            clock(*at_ms),
            delay_ms / 1_000,
            tag(emergency)
        ),
        Output::Departed {
            flight,
            aircraft,
            runway,
            at_ms,
            delay_ms,
            emergency,
            ..
        } => format!(
            "{} {flight} ({aircraft}) departed from {runway}, delayed {} s{}",
            clock(*at_ms),
            delay_ms / 1_000,
            tag(emergency)
        ),
        Output::Diverted { flight, at_ms, .. } => {
            format!("{} {flight} diverted after holding too long", clock(*at_ms))
        }
        Output::EmergencyDeclared {
            flight,
            at_ms,
            kind,
        } => format!("{} {flight} declared {kind:?}", clock(*at_ms)),
        Output::RunwayClosed {
            runway,
            at_ms,
            reason,
        } => format!("{} runway {runway} closed: {reason:?}", clock(*at_ms)),
        Output::RunwayOpened {
            runway,
            at_ms,
            reason,
            open,
        } => format!(
            "{} runway {runway} {}: {reason:?} over",
            clock(*at_ms),
            if *open { "open" } else { "still closed" }
        ),
        Output::RunwayInspection {
            runway,
            from_ms,
            until_ms,
        } => format!(
            "{} runway {runway} shut for inspection until {}",
            clock(*from_ms),
            clock(*until_ms)
        ),
        Output::ActiveRunways { at_ms, runways } => {
            format!("{} runways in use: {}", clock(*at_ms), runways.join(", "))
        }
        Output::WindChanged {
            at_ms,
            direction_deg,
            speed_kt,
        } => format!("{} wind {direction_deg:03} at {speed_kt} kt", clock(*at_ms)),
        Output::GroundStopStarted { at_ms, reason } => {
            format!("{} ground stop: {reason:?}", clock(*at_ms))
        }
        Output::GroundStopLifted {
            at_ms,
            reason,
            clear,
        } => format!(
            "{} ground stop for {reason:?} lifted{}",
            clock(*at_ms),
            if *clear { "" } else { ", another remains" }
        ),
    })
}

#[cfg(test)]
mod tests {
    use host::airport::{Emergency, Reason, RejectReason};

    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn clock_formats_simulated_time() {
        assert_eq!(clock(0), "00:00:00");
        assert_eq!(clock(999), "00:00:00");
        assert_eq!(clock(3_661_000), "01:01:01");
        assert_eq!(clock(86_399_999), "23:59:59");
        // Hours are not wrapped at a day.
        assert_eq!(clock(360_000_000), "100:00:00");
    }

    #[test]
    fn options_have_defaults() {
        let options = parse(&[]).unwrap();
        assert_eq!(options.demo, DemoOptions::default());
        assert_eq!(options.demo.seed, 1);
        assert_eq!(options.demo.hours, 6);
        assert_eq!(options.demo.arrivals_per_hour, 40);
        assert_eq!(options.demo.departures_per_hour, 40);
        assert!(!options.json && !options.quiet);
        assert_eq!(options.file, None);
    }

    #[test]
    fn each_option_is_read() {
        let options = parse(&args(&[
            "--seed",
            "7",
            "--hours",
            "2",
            "--arrivals",
            "10",
            "--departures",
            "5",
            "--json",
            "--quiet",
            "file.json",
        ]))
        .unwrap();
        assert_eq!(
            options.demo,
            DemoOptions {
                seed: 7,
                hours: 2,
                arrivals_per_hour: 10,
                departures_per_hour: 5,
            }
        );
        assert!(options.json && options.quiet);
        assert_eq!(options.file.as_deref(), Some("file.json"));
    }

    #[test]
    fn a_number_option_needs_a_number() {
        for list in [
            &["--seed"][..],
            &["--seed", "x"],
            &["--seed", "-5"],
            &["--seed", "--json"],
        ] {
            let Err(error) = parse(&args(list)) else {
                panic!("{list:?} was accepted");
            };
            assert!(error.starts_with("--seed needs a whole number"), "{error}");
        }
    }

    #[test]
    fn an_hour_count_must_fit() {
        let Err(error) = parse(&args(&["--hours", "4294967296"])) else {
            panic!("accepted");
        };
        assert_eq!(error, "4294967296 is too large");
    }

    #[test]
    fn unknown_and_surplus_arguments_are_refused() {
        for (list, start) in [
            (&["--bogus"][..], "unexpected argument --bogus"),
            (&["a.json", "b.json"], "unexpected argument b.json"),
        ] {
            let Err(error) = parse(&args(list)) else {
                panic!("{list:?} was accepted");
            };
            assert!(error.starts_with(start), "{error}");
        }
    }

    #[test]
    fn a_dash_means_standard_input() {
        assert_eq!(parse(&args(&["-"])).unwrap().file.as_deref(), Some("-"));
    }

    #[test]
    fn no_command_or_an_unknown_one_prints_usage() {
        assert_eq!(run(&[]), Err(USAGE.to_owned()));
        assert_eq!(run(&args(&["fly"])), Err(USAGE.to_owned()));
    }

    #[test]
    fn gen_prints_the_scenario_the_host_builds() {
        let expected = host::demo_scenario_json(DemoOptions {
            seed: 7,
            ..DemoOptions::default()
        }) + "\n";
        assert_eq!(run(&args(&["gen", "--seed", "7"])), Ok(expected));
    }

    #[test]
    fn run_needs_a_file_that_exists() {
        let error = run(&args(&["run"])).unwrap_err();
        assert!(error.starts_with("run needs a scenario file"), "{error}");
        let error = run(&args(&["run", "/no/such/file"])).unwrap_err();
        assert!(error.starts_with("cannot read /no/such/file: "), "{error}");
    }

    #[test]
    fn a_mean_delay_with_no_flights_is_zero() {
        let empty = r#"{"airport":{"name":"X","runways":[{"low":"09","high":"27","heading_deg":90,"length_m":3000}]},"commands":[]}"#;
        let result = host::run_scenario(empty).unwrap();
        let text = report(&result, true);
        assert!(text.contains("landed       0 (mean delay 0 s)"), "{text}");
        assert!(text.contains("departed     0 (mean delay 0 s)"), "{text}");
    }

    fn landed(emergency: Option<Emergency>) -> Output {
        Output::Landed {
            flight: "BAW1".into(),
            aircraft: "A320".into(),
            runway: "27L".into(),
            at_ms: 60_000,
            vacated_ms: 110_000,
            delay_ms: 12_000,
            emergency,
        }
    }

    #[test]
    fn every_output_has_its_line() {
        let line = |output: Output| describe(&output).unwrap();
        assert_eq!(
            describe(&Output::Accepted {
                flight: "A".into(),
                at_ms: 0
            }),
            None
        );
        assert_eq!(
            line(landed(Some(Emergency::Mayday))),
            "00:01:00 BAW1 (A320) landed on 27L, delayed 12 s [mayday]"
        );
        assert!(line(landed(Some(Emergency::PanPan))).ends_with(" [panpan]"));
        assert!(line(landed(None)).ends_with("delayed 12 s"));
        assert_eq!(
            line(Output::Departed {
                flight: "D".into(),
                aircraft: "B738".into(),
                runway: "09".into(),
                at_ms: 0,
                vacated_ms: 40_000,
                delay_ms: 0,
                emergency: None,
            }),
            "00:00:00 D (B738) departed from 09, delayed 0 s"
        );
        assert_eq!(
            line(Output::WindChanged {
                at_ms: 0,
                direction_deg: 5,
                speed_kt: 10
            }),
            "00:00:00 wind 005 at 10 kt"
        );
        assert!(
            line(Output::RunwayOpened {
                runway: "09/27".into(),
                at_ms: 0,
                reason: Reason::Storm,
                open: false
            })
            .contains("still closed")
        );
        assert!(
            line(Output::GroundStopLifted {
                at_ms: 0,
                reason: Reason::Storm,
                clear: false
            })
            .ends_with(", another remains")
        );
        assert!(
            line(Output::Rejected {
                subject: "A".into(),
                at_ms: 0,
                reason: RejectReason::DuplicateFlight
            })
            .contains("rejected: DuplicateFlight")
        );
        assert_eq!(
            line(Output::RunwayInspection {
                runway: "09/27".into(),
                from_ms: 60_000,
                until_ms: 960_000
            }),
            "00:01:00 runway 09/27 shut for inspection until 00:16:00"
        );
        assert_eq!(
            line(Output::ActiveRunways {
                at_ms: 0,
                runways: vec!["27R".into(), "27L".into()]
            }),
            "00:00:00 runways in use: 27R, 27L"
        );
        for output in [
            Output::Diverted {
                flight: "A".into(),
                at_ms: 0,
                reason: host::airport::command::DivertReason::HoldLimit,
            },
            Output::EmergencyDeclared {
                flight: "A".into(),
                at_ms: 0,
                kind: Emergency::Mayday,
            },
            Output::RunwayClosed {
                runway: "09/27".into(),
                at_ms: 0,
                reason: Reason::Storm,
            },
            Output::GroundStopStarted {
                at_ms: 0,
                reason: Reason::Storm,
            },
        ] {
            assert!(line(output).starts_with("00:00:00 "));
        }
    }
}
