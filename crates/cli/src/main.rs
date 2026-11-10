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
