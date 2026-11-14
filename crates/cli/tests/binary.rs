//! Runs the built `airport-sim` program as a user would.

use std::process::{Command, Output};

fn airport_sim(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_airport-sim"))
        .args(args)
        .output()
        .expect("airport-sim runs")
}

fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn the_program_exits_with_the_right_code() {
    let output = airport_sim(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("usage: airport-sim"));

    let output = airport_sim(&["demo", "--quiet", "--hours", "1"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn quiet_prints_only_the_summary() {
    let text = stdout(&airport_sim(&["demo", "--quiet", "--hours", "1"]));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 8, "{text}");
    assert!(lines[0].starts_with("ended at"));
    let fingerprint = lines[7].strip_prefix("fingerprint  ").unwrap();
    assert_eq!(fingerprint.len(), 16);
    assert!(fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn demo_and_gen_then_run_agree() {
    let options = ["--seed", "7", "--hours", "1"];
    let scenario = stdout(&airport_sim(&[&["gen"][..], &options].concat()));
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("seed-7.json");
    std::fs::write(&path, scenario).unwrap();
    let from_file = stdout(&airport_sim(&["run", path.to_str().unwrap(), "--json"]));
    let demo = stdout(&airport_sim(
        &[&["demo"][..], &options, &["--json"]].concat(),
    ));
    assert_eq!(from_file, demo);
}
