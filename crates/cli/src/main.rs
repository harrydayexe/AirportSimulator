//! `airport-sim`: command-line wrapper around the host.

use std::process::ExitCode;

use host::airport;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (distance, speed) = match args.as_slice() {
        [] => (5, 140),
        [d, s] => match (d.parse(), s.parse()) {
            (Ok(d), Ok(s)) => (d, s),
            _ => return usage(),
        },
        _ => return usage(),
    };
    match airport::gap_ms(distance, speed) {
        Some(ms) => {
            println!("{distance} nm at {speed} kt = {ms} ms");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("speed must be greater than zero");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: airport-sim [DISTANCE_NM SPEED_KT]");
    ExitCode::FAILURE
}
