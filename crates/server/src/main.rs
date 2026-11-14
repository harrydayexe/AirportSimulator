//! `airport-server`: runs one replica. See the library documentation.

use std::process::ExitCode;

use airport_server::config::{USAGE, parse};
use airport_server::{http, server};

/// The release this binary was built for, or the crate version.
const VERSION: &str = match option_env!("AIRPORT_SIM_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// This machine's name, as far as it can be told without asking the system.
fn hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// Exit code 0 if the replica at `address` says it is alive.
fn healthcheck(address: &str) -> ExitCode {
    match http::request(address, "GET", "/healthz", "") {
        Ok((200, _)) => ExitCode::SUCCESS,
        Ok((status, body)) => {
            eprintln!("{address} answered {status}: {body}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("cannot reach {address}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help" | "-h" | "help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("--version" | "-V") => {
            println!("airport-server {VERSION}");
            return ExitCode::SUCCESS;
        }
        Some("healthcheck") => {
            return healthcheck(args.get(1).map_or("127.0.0.1:8080", String::as_str));
        }
        _ => {}
    }
    let options = match parse(
        &args,
        &|name| std::env::var(name).ok(),
        hostname().as_deref(),
    ) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let Err(message) = server::run(&options) else {
        return ExitCode::SUCCESS;
    };
    eprintln!("airport-server: {message}");
    ExitCode::FAILURE
}
