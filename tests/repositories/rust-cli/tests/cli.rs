use std::process::Command as Process;

use rust_cli::{parse_args, run, Command};

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn greet_end_to_end_through_the_library() {
    let cmd = parse_args(argv(&["rust-cli", "greet", "Grace"])).unwrap();
    assert_eq!(run(&cmd).unwrap(), "Hello, Grace!");
}

#[test]
fn shout_joins_all_arguments() {
    let cmd = Command { name: "shout".into(), args: argv(&["make", "it", "loud"]) };
    assert_eq!(run(&cmd).unwrap(), "MAKE IT LOUD!");
}

#[test]
fn binary_prints_greeting() {
    let output = Process::new(env!("CARGO_BIN_EXE_rust-cli"))
        .args(["greet", "Linus"])
        .output()
        .expect("binary runs");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "Hello, Linus!");
}

#[test]
fn binary_reports_unknown_command() {
    let output = Process::new(env!("CARGO_BIN_EXE_rust-cli"))
        .arg("juggle")
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown command: juggle"));
}
